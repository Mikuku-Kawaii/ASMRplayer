//! Source precision comes from file headers, never from mpv's decoded sample format.
use std::time::Duration;
use tokio::io::AsyncReadExt;

const HEADER_LIMIT: usize = 64 * 1024;

pub async fn local_bit_depth(path: &str) -> Option<u16> {
    let file = tokio::fs::File::open(path).await.ok()?;
    let mut bytes = Vec::new();
    file.take(HEADER_LIMIT as u64)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    header_bit_depth(&bytes)
}

pub async fn online_bit_depth(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Option<u16> {
    let mut request = client.get(url).timeout(Duration::from_secs(4));
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let mut response = request
        .header(
            reqwest::header::RANGE,
            format!("bytes=0-{}", HEADER_LIMIT - 1),
        )
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .ok()?;
    match response.status() {
        reqwest::StatusCode::OK => {} // Some servers ignore Range; still read only the bounded prefix.
        reqwest::StatusCode::PARTIAL_CONTENT => {
            if !response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)?
                .to_str()
                .ok()?
                .starts_with("bytes 0-")
            {
                return None;
            }
        }
        _ => return None,
    }
    let mut bytes = Vec::new();
    while bytes.len() < HEADER_LIMIT {
        let chunk = response.chunk().await.ok()??;
        bytes.extend_from_slice(&chunk[..chunk.len().min(HEADER_LIMIT - bytes.len())]);
        if let Some(depth) = header_bit_depth(&bytes) {
            return Some(depth);
        }
    }
    None
}

fn le16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn header_bit_depth(bytes: &[u8]) -> Option<u16> {
    // FLAC STREAMINFO carries the original bits-per-sample even when decoded as s32.
    if bytes.starts_with(b"fLaC") {
        if bytes.get(4)? & 0x7f != 0 || bytes.get(5..8)? != [0, 0, 34] {
            return None;
        }
        bytes.get(..42)?;
        return Some((((bytes[20] & 1) as u16) << 4 | (bytes[21] >> 4) as u16) + 1);
    }
    let wave =
        (bytes.starts_with(b"RIFF") || bytes.starts_with(b"RF64")) && bytes.get(8..12)? == b"WAVE";
    let aiff = bytes.starts_with(b"FORM") && matches!(bytes.get(8..12)?, b"AIFF" | b"AIFC");
    if !wave && !aiff {
        return None;
    }
    let mut offset = 12usize;
    while let Some(chunk) = bytes.get(offset..offset.checked_add(8)?) {
        let raw_size: [u8; 4] = chunk[4..8].try_into().ok()?;
        let size = if wave {
            u32::from_le_bytes(raw_size)
        } else {
            u32::from_be_bytes(raw_size)
        } as usize;
        let start = offset.checked_add(8)?;
        if wave && &chunk[..4] == b"fmt " {
            let fmt = bytes.get(start..start.checked_add(size)?)?;
            if fmt.len() < 16 {
                return None;
            }
            let mut format = le16(fmt, 0)?;
            let storage_bits = le16(fmt, 14)?;
            let mut bits = storage_bits;
            if format == 0xfffe {
                if fmt.len() < 40 || le16(fmt, 16)? < 22 {
                    return None;
                }
                // Only PCM/IEEE_FLOAT subformat GUIDs have a meaningful linear bit depth.
                if fmt.get(26..40)? != [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113] {
                    return None;
                }
                format = le16(fmt, 24)?;
                let valid_bits = le16(fmt, 18)?;
                if valid_bits > 0 {
                    bits = valid_bits;
                }
            }
            return (matches!(format, 1 | 3)
                && bits > 0
                && bits <= storage_bits
                && storage_bits <= 64)
                .then_some(bits);
        }
        if aiff && &chunk[..4] == b"COMM" {
            let comm = bytes.get(start..start.checked_add(size)?)?;
            if comm.len() < 18 {
                return None;
            }
            if bytes.get(8..12)? == b"AIFC"
                && !matches!(comm.get(18..22)?, b"NONE" | b"sowt" | b"fl32" | b"fl64")
            {
                return None;
            }
            let bits = u16::from_be_bytes(comm.get(6..8)?.try_into().ok()?);
            return (bits > 0 && bits <= 64).then_some(bits);
        }
        offset = start.checked_add(size)?.checked_add(size % 2)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    fn wav(bits: u16, valid: Option<u16>, format: u16) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        let size = if valid.is_some() { 40u32 } else { 16u32 };
        bytes.extend(size.to_le_bytes());
        bytes.extend(if valid.is_some() { 0xfffeu16 } else { format }.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(48000u32.to_le_bytes());
        bytes.extend((48000u32 * 2 * u32::from(bits) / 8).to_le_bytes());
        bytes.extend((2 * bits / 8).to_le_bytes());
        bytes.extend(bits.to_le_bytes());
        if let Some(valid) = valid {
            bytes.extend(22u16.to_le_bytes());
            bytes.extend(valid.to_le_bytes());
            bytes.extend(3u32.to_le_bytes());
            bytes.extend(format.to_le_bytes());
            bytes.extend([0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
        }
        bytes
    }

    #[test]
    fn source_depth_reads_pcm_valid_bits_and_flac_streaminfo() {
        for bits in [8, 16, 24, 32] {
            assert_eq!(header_bit_depth(&wav(bits, None, 1)), Some(bits));
            let mut flac = vec![0; 42];
            flac[..8].copy_from_slice(b"fLaC\x80\0\0\x22");
            flac[20] = ((bits - 1) >> 4) as u8;
            flac[21] = (((bits - 1) & 15) << 4) as u8;
            assert_eq!(header_bit_depth(&flac), Some(bits));
            assert_eq!(header_bit_depth(&flac[..30]), None);
        }
        assert_eq!(header_bit_depth(&wav(32, Some(24), 1)), Some(24));
        assert_eq!(header_bit_depth(&wav(32, None, 3)), Some(32));
        let mut with_junk = b"RIFF\0\0\0\0WAVEJUNK\x01\0\0\0\0\0".to_vec();
        with_junk.extend_from_slice(&wav(24, None, 1)[12..]);
        assert_eq!(header_bit_depth(&with_junk), Some(24));
    }

    #[test]
    fn source_depth_does_not_invent_precision_for_lossy_or_malformed_files() {
        assert_eq!(header_bit_depth(b"ID3\0\0\0\0\0\0\0"), None);
        assert_eq!(header_bit_depth(&wav(4, None, 2)), None); // ADPCM is not linear PCM precision.
        assert_eq!(header_bit_depth(&wav(16, Some(24), 1)), None);
        let wave = wav(24, None, 1);
        for length in 0..wave.len() {
            assert_eq!(header_bit_depth(&wave[..length]), None);
        }
        let mut invalid_guid = wav(32, Some(24), 1);
        invalid_guid[59] = 0;
        assert_eq!(header_bit_depth(&invalid_guid), None);
        let mut aiff = b"FORM\0\0\0\0AIFFCOMM\0\0\0\x12".to_vec();
        aiff.extend([0u8; 18]);
        aiff[27] = 24;
        assert_eq!(header_bit_depth(&aiff), Some(24));
    }

    #[tokio::test]
    async fn source_depth_reads_local_header_without_decoding_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("precision.wav");
        std::fs::write(&path, wav(32, Some(24), 1)).unwrap();
        assert_eq!(local_bit_depth(path.to_str().unwrap()).await, Some(24));
    }

    #[tokio::test]
    async fn source_depth_online_accepts_zero_range_and_bounded_200_but_rejects_wrong_offset() {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (status, range, expected) in [
            (
                "206 Partial Content",
                "Content-Range: bytes 0-59/100000000\r\n",
                Some(24),
            ),
            ("200 OK", "", Some(24)),
            (
                "206 Partial Content",
                "Content-Range: bytes 100-159/100000000\r\n",
                None,
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/audio.wav", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let count = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
                assert!(request.contains("range: bytes=0-65535"));
                assert!(request.contains("x-source-test: present"));
                socket.write_all(format!("HTTP/1.1 {status}\r\n{range}Content-Length: 100000000\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                socket.write_all(&wav(32, Some(24), 1)).await.unwrap();
                // Keep the body open: metadata probing must return after the header, not download audio.
                let mut byte = [0];
                let _ = socket.read(&mut byte).await;
            });
            assert_eq!(
                tokio::time::timeout(
                    Duration::from_secs(2),
                    online_bit_depth(&client, &url, &[("X-Source-Test".into(), "present".into())])
                )
                .await
                .unwrap(),
                expected
            );
            server.await.unwrap();
        }
    }
}
