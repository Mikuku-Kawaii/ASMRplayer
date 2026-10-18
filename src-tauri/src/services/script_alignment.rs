//! Japanese scripts use existing ASR cue timestamps; uncertain rows remain unaligned.
use super::{subtitle_tasks::SubtitleTaskService, subtitle_documents::{render_export, write_subtitle, SubtitleFormat, SubtitleMode}};
use crate::{db::subtitles::SubtitleDocument, domain::{AppError, AppResult, ErrorCode, SubtitleCueDto}};
use serde::Serialize;
use std::{collections::BTreeSet, io::Read, path::Path};
fn invalid(message: &str)->AppError { AppError::new(ErrorCode::InvalidInput,message) }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct LocalScript { pub path: String, pub title: String, pub attachment_id: Option<String> }
fn script_folder(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| {
        let part=part.to_lowercase();
        ["腳本", "脚本", "台本", "script"].iter().any(|name| part.contains(name))
    })
}
fn text_file(path: &Path) -> bool {
    path.extension().and_then(|s|s.to_str()).is_some_and(|s|["txt","md"].iter().any(|ext|s.eq_ignore_ascii_case(ext)))
}
pub fn read_local_script(path: &Path) -> AppResult<String> {
    if !text_file(path) { return Err(invalid("请选择 TXT 或 MD 台本文件")); }
    let file=std::fs::File::open(path).map_err(|_|invalid("无法读取台本文件，请检查文件是否存在"))?;
    let mut bytes=Vec::new();
    file.take(100_001).read_to_end(&mut bytes).map_err(|_|invalid("无法读取台本文件"))?;
    decode_script_bytes(&bytes)
}
fn decode_script_bytes(bytes: &[u8]) -> AppResult<String> {
    if bytes.len()>100_000 { return Err(invalid("台本文件最多 100 KB")); }
    let text=if let Some((encoding,bom))=encoding_rs::Encoding::for_bom(&bytes) {
        let (text,errors)=encoding.decode_without_bom_handling(&bytes[bom..]);
        if errors { return Err(invalid("台本编码损坏，请转换为 UTF-8 后重试")); }
        text.into_owned()
    } else if let Ok(text)=std::str::from_utf8(&bytes) { text.to_owned() }
    else {
        let (text,errors)=encoding_rs::SHIFT_JIS.decode_without_bom_handling(&bytes);
        if errors { return Err(invalid("无法识别台本编码，支持 UTF-8、UTF-16 和 Shift-JIS")); }
        text.into_owned()
    };
    if text.contains('\0') { return Err(invalid("文件包含空字符，请选择文本台本")); }
    Ok(text.replace("\r\n","\n"))
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct ScriptRow { line: usize, text: String, start_ms: Option<i64>, end_ms: Option<i64>, score: u32 }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct ScriptPreview { revision: String, rows: Vec<ScriptRow>, matched: usize }
fn normalized(text: &str)->String { text.chars().filter(|c| !c.is_whitespace() && !super::speech_engine::is_punctuation(*c)).collect() }
fn pairs(text: &str)->BTreeSet<(char,char)> { text.chars().collect::<Vec<_>>().windows(2).map(|p|(p[0],p[1])).collect() }
fn similarity(left:&str,right:&str)->f64 {
    if left==right { return 1.0; }
    let l:Vec<_>=left.chars().collect(); let r:Vec<_>=right.chars().collect();
    if l.len()<5 || r.len()<5 || l.len().min(r.len())*4<l.len().max(r.len())*3
        || l[..2]!=r[..2] || l[l.len()-2..]!=r[r.len()-2..] { return 0.0; }
    let a=pairs(left); let b=pairs(right);
    2.0*a.intersection(&b).count() as f64/(a.len()+b.len()).max(1) as f64
}
fn align(doc:&SubtitleDocument, script:&str)->AppResult<ScriptPreview> {
    if !doc.source_key.starts_with("sensevoice:") || !doc.recognition_complete || !doc.reference_subtitles.is_empty() {
        return Err(invalid("请先完成整轨日语识别，再对齐无时间戳台本；中文字幕分段识别不作为整轨锚点"));
    }
    if script.len()>100_000 || script.contains('\0') { return Err(invalid("台本最多 100 KB，不能含空字符")); }
    let lines:Vec<_>=script.lines().map(str::trim).filter(|line|!line.is_empty()).collect();
    if lines.is_empty() || lines.len()>1000 || lines.iter().any(|s|s.chars().count()>500) {
        return Err(invalid("请按台词分行粘贴日文台本，最多 1000 行、每行 500 字"));
    }
    if !script.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)) { return Err(invalid("此入口仅用于日文原文台本")); }
    let cues=doc.effective_original();
    let texts:Vec<_>=cues.iter().map(|c|normalized(&c.text)).collect();
    let mut cursor=0;
    let mut rows=Vec::new();
    for (line,text) in lines.into_iter().enumerate() {
        let wanted=normalized(text);
        let mut candidates=Vec::new();
        // Preserve order; a missing/ambiguous line never advances the audio cursor.
        for begin in cursor..(cursor+40).min(cues.len()) {
            let mut joined=String::new(); let mut best=(0.0,begin);
            for end in begin..(begin+8).min(cues.len()) {
                joined.push_str(&texts[end]);
                if joined.chars().count()>wanted.chars().count()*2+8 { break; }
                let score=similarity(&wanted,&joined);
                if score>best.0 { best=(score,end); }
            }
            if best.0>=0.85 { candidates.push((best.0,begin,best.1)); }
        }
        candidates.sort_by(|a,b|b.0.total_cmp(&a.0));
        let best=candidates.first().copied().filter(|best|candidates.get(1).is_none_or(|other|best.0-other.0>=0.08));
        let (start_ms,end_ms,score)=if let Some((score,begin,end))=best {
            cursor=end+1;
            (Some(cues[begin].start_ms),Some(cues[end].end_ms),(score*100.0).round() as u32)
        } else { (None,None,0) };
        rows.push(ScriptRow{line:line+1,text:text.into(),start_ms,end_ms,score});
    }
    let matched=rows.iter().filter(|r|r.start_ms.is_some()).count();
    Ok(ScriptPreview{revision:doc.revision(),rows,matched})
}
impl SubtitleTaskService {
    pub fn list_local_scripts(&self, version: &str) -> AppResult<Vec<LocalScript>> {
        let mut scripts=self.db.with_conn(|conn| {
            let mut stmt=conn.prepare("SELECT a.absolute_path,COALESCE(a.relative_path,a.title) FROM attachments a JOIN tracks t ON t.work_id=a.work_id JOIN track_versions v ON v.track_id=t.id WHERE v.id=?1 AND a.attachment_type='text' AND a.availability='available' AND a.absolute_path IS NOT NULL")?;
            let rows=stmt.query_map([version],|r|Ok(LocalScript{path:r.get(0)?,title:r.get(1)?,attachment_id:None}))?.collect::<Result<Vec<_>,_>>()?;
            Ok(rows)
        })?;
        scripts.retain(|s|text_file(Path::new(&s.path)) && Path::new(&s.path).is_file());
        scripts.sort_by(|a,b|script_folder(&b.title).cmp(&script_folder(&a.title)).then_with(||a.title.cmp(&b.title)));
        scripts.dedup_by(|a,b|a.path==b.path);
        Ok(scripts)
    }
    pub async fn list_work_scripts(&self, version: &str) -> AppResult<Vec<LocalScript>> {
        use rusqlite::OptionalExtension;
        let work: Option<(String,bool)> = self.db.with_conn(|c| Ok(c.query_row(
            "SELECT t.work_id,EXISTS(SELECT 1 FROM media_sources s WHERE s.version_id=v.id AND s.source_kind='online_provider')
             FROM tracks t JOIN track_versions v ON v.track_id=t.id WHERE v.id=?1",[version],|r|Ok((r.get(0)?,r.get(1)?))).optional()?))?;
        let Some((work,online))=work else { return Ok(vec![]); };
        if online { self.provider.ensure_work_download_files(&work,None).await?; }
        let mut scripts=self.list_local_scripts(version)?;
        let remote: Vec<(String,String,Option<String>)> = self.db.with_conn(|c| {
            let mut stmt=c.prepare("SELECT id,COALESCE(relative_path,title),absolute_path FROM attachments WHERE work_id=?1 AND attachment_type='text' AND provider IS NOT NULL AND remote_file_id IS NOT NULL AND availability='available'
                AND (version_id IS NULL OR version_id=?2)")?;
            let rows=stmt.query_map(rusqlite::params![work,version],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            Ok(rows.collect::<Result<_,_>>()?)
        })?;
        for (id,title,local) in remote {
            if !text_file(Path::new(&title)) || local.is_some_and(|p|Path::new(&p).is_file()) { continue; }
            scripts.push(LocalScript{path:format!("online:{id}"),title,attachment_id:Some(id)});
        }
        scripts.sort_by(|a,b|script_folder(&b.title).cmp(&script_folder(&a.title)).then_with(||a.title.cmp(&b.title)));
        Ok(scripts)
    }
    pub async fn read_online_script(&self, version: &str, attachment: &str) -> AppResult<String> {
        use rusqlite::OptionalExtension;
        use futures_util::StreamExt;
        let row: Option<(String,String,String,String,Option<String>,Option<i64>)> = self.db.with_conn(|c| Ok(c.query_row(
            "SELECT a.provider,a.source_work_id,a.remote_file_id,COALESCE(a.relative_path,a.title),a.absolute_path,a.byte_size
             FROM attachments a JOIN tracks t ON t.work_id=a.work_id JOIN track_versions v ON v.track_id=t.id
             WHERE a.id=?1 AND v.id=?2 AND a.attachment_type='text' AND a.availability='available' AND (a.version_id IS NULL OR a.version_id=v.id)",
            rusqlite::params![attachment,version],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?))?;
        let Some((provider,source_work_id,remote_file_id,title,local,size))=row else { return Err(invalid("未找到当前作品的原文台本")); };
        if !text_file(Path::new(&title)) { return Err(invalid("请选择 TXT 或 MD 原文台本")); }
        if let Some(path)=local.filter(|p|Path::new(p).is_file()) { return read_local_script(Path::new(&path)); }
        if size.is_some_and(|n|n>100_000) { return Err(invalid("台本文件最多 100 KB")); }
        let media=self.provider.resolve_media(&crate::providers::ProviderMediaRef{provider,source_work_id,remote_file_id,remote_relative_path:title},crate::providers::MediaPurpose::Download,false).await?;
        let client=self.provider.build_download_client()?;
        let mut request=client.get(media.url);
        for (key,value) in media.headers { request=request.header(key,value); }
        let response=request.send().await.map_err(|_|invalid("在线原文台本读取失败，请检查网络后重试"))?;
        if !response.status().is_success() { return Err(invalid("在线原文台本暂不可用，请稍后重试")); }
        if response.content_length().is_some_and(|n|n>100_000) { return Err(invalid("台本文件最多 100 KB")); }
        let mut bytes=Vec::new(); let mut stream=response.bytes_stream();
        while let Some(chunk)=stream.next().await {
            let chunk=chunk.map_err(|_|invalid("在线原文台本传输中断，请重试"))?;
            if bytes.len()+chunk.len()>100_000 { return Err(invalid("台本文件最多 100 KB")); }
            bytes.extend_from_slice(&chunk);
        }
        decode_script_bytes(&bytes)
    }
    pub fn preview_script_alignment(&self,version:&str,script:&str)->AppResult<ScriptPreview> {
        self.with_idle(|| {
            let key=self.db.audio_subtitle_key(version)?;
            let doc=self.db.subtitle_document(version,&key)?.ok_or_else(||invalid("请先完成整轨日语识别"))?;
            align(&doc,script)
        })
    }
    pub fn export_script_alignment(&self,version:&str,script:&str,revision:&str,path:&Path)->AppResult<usize> {
        self.with_idle(|| {
            if path.extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case("srt")) { return Err(invalid("请导出为 SRT 文件")); }
            let key=self.db.audio_subtitle_key(version)?;
            let doc=self.db.subtitle_document(version,&key)?.ok_or_else(||invalid("识别锚点已变化，请重新预览"))?;
            if doc.revision()!=revision { return Err(invalid("字幕已变化，请重新预览对齐")); }
            let preview=align(&doc,script)?;
            if preview.matched==0 { return Err(invalid("没有可导出的匹配段落")); }
            let cues=preview.rows.into_iter().filter_map(|r|Some(SubtitleCueDto{index:r.line,start_ms:r.start_ms?,end_ms:r.end_ms?,text:r.text})).collect();
            let aligned=SubtitleDocument::new(version.into(),"external:script-export".into(),cues,true);
            let (text,_)=render_export(&aligned,SubtitleFormat::Srt,SubtitleMode::Original)?;
            write_subtitle(path,text.as_bytes())?;
            Ok(preview.matched)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn online_original_scripts_are_scoped_decoded_and_never_imported_as_timed_subtitles() {
        use crate::{db::Database,providers::{AsmrOneProvider,parse_tracks_v2_json,parse_work_info_json},services::{recognition::RecognitionService,playback::PlaybackService}};
        use wiremock::{Mock,MockServer,ResponseTemplate,matchers::{method,path}};
        let server=MockServer::start().await;
        let dir=tempfile::tempdir().unwrap();
        let db=Database::open_and_migrate(&dir.path().join("data"),&dir.path().join("cache")).unwrap();
        let provider=AsmrOneProvider::new(db.clone()); provider.set_api_base(&server.uri()).unwrap();
        let (bytes,_,_)=encoding_rs::SHIFT_JIS.encode("こんばんは\r\nおやすみなさい");
        let raw=serde_json::json!([
            {"type":"audio","title":"01.wav","hash":"audio"},
            {"type":"text","title":"05_腳本/01.txt","hash":"jp","size":bytes.len(),"mediaDownloadUrl":format!("{}/script",server.uri())},
            {"type":"text","title":"readme.md","hash":"readme"},
            {"type":"text","title":"中文.srt","hash":"chinese"}
        ]).to_string();
        let work=parse_work_info_json(include_str!("../../../tests/fixtures/provider/work_info_sample.json")).unwrap();
        let work_id=provider.upsert_online_work_in_db(&work,&parse_tracks_v2_json("1705068",&raw).unwrap(),&raw).unwrap();
        let version=db.get_work_detail(&work_id).unwrap().tracks[0].versions[0].id.clone();
        Mock::given(method("GET")).and(path("/api/tracks/1705068")).respond_with(ResponseTemplate::new(200).set_body_string(raw)).mount(&server).await;
        Mock::given(method("GET")).and(path("/script")).respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.into_owned())).expect(1).mount(&server).await;
        let recognition=RecognitionService::new(db.clone(),dir.path().join("models"),"script-online-test");
        let service=SubtitleTaskService::new(db.clone(),recognition,PlaybackService::new(db.clone(),provider.clone()),provider);
        let scripts=service.list_work_scripts(&version).await.unwrap();
        assert_eq!(scripts.len(),2);
        assert_eq!(scripts[0].title,"05_腳本/01.txt");
        let id=scripts[0].attachment_id.as_ref().unwrap();
        assert!(service.read_online_script("other-work-version",id).await.is_err());
        assert_eq!(service.read_online_script(&version,id).await.unwrap(),"こんばんは\nおやすみなさい");
        db.with_conn(|c| {
            assert_eq!(c.query_row("SELECT COUNT(*) FROM subtitle_documents",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(c.query_row("SELECT COUNT(*) FROM attachments WHERE absolute_path IS NOT NULL",[],|r|r.get::<_,i64>(0))?,0);
            c.execute("UPDATE attachments SET byte_size=100001 WHERE id=?1",[id])?;
            Ok(())
        }).unwrap();
        assert!(service.read_online_script(&version,id).await.unwrap_err().message.contains("100 KB"));
    }
    #[test]
    fn t1513_local_script_reads_utf8_utf16_and_shift_jis_without_changing_files() {
        let dir=tempfile::tempdir().unwrap(); let path=dir.path().join("台本.txt");
        let text="おやすみなさい\r\nこんばんは";
        let (sjis,_,_)=encoding_rs::SHIFT_JIS.encode(text);
        let utf16:Vec<u8>=[0xff,0xfe].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        for bytes in [text.as_bytes().to_vec(),utf16,sjis.into_owned()] {
            std::fs::write(&path,&bytes).unwrap();
            assert_eq!(read_local_script(&path).unwrap(),text.replace("\r\n","\n"));
            assert_eq!(std::fs::read(&path).unwrap(),bytes);
        }
        std::fs::write(&path,vec![b'a';100_001]).unwrap();
        assert!(read_local_script(&path).is_err());
        assert!(read_local_script(&dir.path().join("台本.pdf")).is_err());
    }
    #[test]
    fn t1513_local_script_discovery_scopes_to_work_and_prioritizes_script_folder() {
        use crate::{db::Database,providers::AsmrOneProvider,services::{recognition::RecognitionService,playback::PlaybackService}};
        let dir=tempfile::tempdir().unwrap();
        let db=Database::open_and_migrate(&dir.path().join("data"),&dir.path().join("cache")).unwrap();
        db.with_conn(|c| {
            c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','work','now','now'),('other','other','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','track','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?;
            for (id,work,path) in [("1","w","00_readme.txt"),("2","w","05_腳本/01.txt"),("3","other","05_腳本/other.txt"),("4","w","missing.txt")] {
                let abs=dir.path().join(path);
                if id!="4" { std::fs::create_dir_all(abs.parent().unwrap()).unwrap(); std::fs::write(&abs,"こんばんは").unwrap(); }
                c.execute("INSERT INTO attachments(id,work_id,attachment_type,title,relative_path,absolute_path,availability,created_at) VALUES(?1,?2,'text',?3,?3,?4,'available','now')",rusqlite::params![id,work,path,abs.to_string_lossy()])?;
            }
            Ok(())
        }).unwrap();
        let provider=AsmrOneProvider::new(db.clone());
        let recognition=RecognitionService::new(db.clone(),dir.path().join("data"),"script-test");
        let playback=PlaybackService::new(db.clone(),provider.clone());
        let service=SubtitleTaskService::new(db,recognition,playback,provider);
        let scripts=service.list_local_scripts("v").unwrap();
        assert_eq!(scripts.iter().map(|s|s.title.as_str()).collect::<Vec<_>>(),["05_腳本/01.txt","00_readme.txt"]);
        assert!(service.list_local_scripts("unknown").unwrap().is_empty());
    }
    #[test]
    fn t1513_script_alignment_uses_audio_anchors_preserves_script_and_marks_ambiguity() {
        let doc=SubtitleDocument::new("v".into(),"sensevoice:test".into(),vec![
            SubtitleCueDto{index:1,start_ms:100,end_ms:1000,text:"こんばんは".into()},
            SubtitleCueDto{index:2,start_ms:1500,end_ms:2100,text:"おやすみ".into()},
            SubtitleCueDto{index:3,start_ms:2200,end_ms:2700,text:"なさい".into()},
        ],true);
        let preview=align(&doc,"こんばんは\nおやすみなさい\n未収録の台詞").unwrap();
        assert_eq!(preview.matched,2);
        assert_eq!((preview.rows[1].start_ms,preview.rows[1].end_ms),(Some(1500),Some(2700)));
        assert!(preview.rows[2].start_ms.is_none());
        assert_eq!(preview.rows[1].text,"おやすみなさい");
        let mut repeated=doc.clone(); repeated.original.push(SubtitleCueDto{index:4,start_ms:3000,end_ms:3900,text:"こんばんは".into()});
        assert_eq!(align(&repeated,"こんばんは").unwrap().matched,0);
        let mut partial=doc; partial.recognition_complete=false;
        assert!(align(&partial,"こんばんは").is_err());
    }
}
