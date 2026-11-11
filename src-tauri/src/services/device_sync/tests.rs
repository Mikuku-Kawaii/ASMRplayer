use super::*;
use rusqlite::params;
#[tokio::test]
async fn receiver_completes_source_metadata_without_changing_affinity() {
    use wiremock::matchers::{header, headers, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let (_dir, service) = fixture("meta");
    service.source.set_api_base(&server.uri()).unwrap();
    service.db.with_conn(|c|{
        c.execute_batch("INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES('source','metaw','asmr_one','1234','now'); INSERT INTO favorites VALUES('metaw','now'); INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES('metav',3210,0,'2026-01-01T00:00:00Z');")?;
        Ok(())
    }).unwrap();
    Mock::given(method("GET"))
        .and(path("/api/workInfo/001234"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let initial = service.clone();
    tokio::task::spawn_blocking(move || initial.complete_source_information().unwrap())
        .await
        .unwrap();
    assert_eq!(
        service
            .db
            .get_setting("device_sync_metadata_pending")
            .unwrap()
            .as_deref(),
        Some("1")
    );
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/workInfo/001234"))
        .and(header("origin", "https://asmr.one"))
        .and(header("referer", "https://asmr.one/"))
        .and(headers("accept", vec!["application/json", "text/plain", "*/*"]))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":1234,"source_id":"VJ00001234","title":"完整作品",
            "circle":{"name":"测试社团"},"vas":[{"id":1,"name":"测试声优"}],
            "tags":[{"id":2,"name":"ASMR"}],"description":"来源简介","release":"2026-11-10",
            "has_subtitle":true
        })))
        .expect(1)
        .mount(&server)
        .await;
    let copy = service.clone();
    tokio::task::spawn_blocking(move || {
        copy.complete_source_information().unwrap();
        copy.complete_source_information().unwrap();
    })
    .await
    .unwrap();
    server.verify().await;
    service.db.with_conn(|c|{
        let row=c.query_row("SELECT title,rj_code,circle_name,voice_actors_json,tags_json,description,has_subtitle_metadata FROM works WHERE id='metaw'",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,bool>(6)?)))?;
        assert_eq!(row,("完整作品".into(),"VJ00001234".into(),"测试社团".into(),"[\"测试声优\"]".into(),"[\"ASMR\"]".into(),"来源简介".into(),true));
        assert_eq!(c.query_row("SELECT count(*) FROM favorites WHERE work_id='metaw'",[],|r|r.get::<_,i64>(0))?,1);
        assert_eq!(c.query_row("SELECT position_ms FROM playback_positions WHERE version_id='metav'",[],|r|r.get::<_,i64>(0))?,3210);
        Ok(())
    }).unwrap();
    assert_eq!(
        service
            .db
            .get_setting("device_sync_metadata_pending")
            .unwrap()
            .as_deref(),
        Some("0")
    );
}
#[test]
fn retired_selection_can_be_opened_without_rebinding_or_losing_the_journal() {
    let (_dir, service) = fixture("retired");
    service
        .db
        .set_setting("device_sync_provider", "\"google\"")
        .unwrap();
    assert!(service.selected().unwrap() == ProviderId::Github);
    assert!(
        service
            .db
            .get_setting("device_sync_provider")
            .unwrap()
            .as_deref()
            == Some("\"google\"")
    );
    service.exchange_local(None, None).unwrap();
    let count = service
        .db
        .with_conn(|c| Ok(c.query_row("SELECT count(*) FROM works", [], |r| r.get::<_, i64>(0))?))
        .unwrap();
    assert_eq!(count, 1);
}
fn fixture(prefix: &str) -> (tempfile::TempDir, SyncService) {
    let dir = tempfile::tempdir().unwrap();
    let db =
        Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache")).unwrap();
    let file = dir.path().join("synthetic.bin");
    std::fs::write(&file, b"synthetic content identity fixture").unwrap();
    db.with_conn(|c|{c.execute("INSERT INTO works(id,title,created_at,updated_at) VALUES(?1,'同步夹具','2026-01-01','2026-01-01')",[format!("{prefix}w")])?;
        c.execute("INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES(?1,?2,'合成音轨','1','2026-01-01')",params![format!("{prefix}t"),format!("{prefix}w")])?;
        c.execute("INSERT INTO track_versions(id,track_id,created_at,duration_ms) VALUES(?1,?2,'2026-01-01',10000)",params![format!("{prefix}v"),format!("{prefix}t")])?;
        c.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES(?1,?2,'local_imported',?3,'bin','2026-01-01','2026-01-01')",params![format!("{prefix}s"),format!("{prefix}v"),file.to_string_lossy()])?;Ok(())}).unwrap();
    (dir, SyncService::new(db))
}

#[test]
fn playlists_roundtrip_order_tombstone_missing_media_and_restart() {
    let (da,a)=fixture("pla");let (_db,b)=fixture("plb");
    let p=a.db.create_playlist("夜雨",vec!["plav".into(),"plav".into()]).unwrap();
    assert_eq!(p.version_ids,vec!["plav"]);
    let reopened=Database::open_and_migrate(&da.path().join("data"),&da.path().join("cache")).unwrap();
    assert_eq!(reopened.list_playlists().unwrap()[0].version_ids,vec!["plav"]);
    assert!(a.db.create_playlist(" ",vec![]).is_err());
    assert!(a.db.update_playlist(&p.id,None,&[],Some("plav"),Some(0),Some(&[]),None).is_err());
    a.db.update_playlist(&p.id,Some("夜雨"),&["plav".into()],None,None,None,None).unwrap();
    assert_eq!(a.db.list_playlists().unwrap()[0].version_ids,vec!["plav"]);
    let doc=a.exchange_local(None,None).unwrap();
    let key=format!("playlist|{}|{}",catalog::hash(p.id.as_bytes()),p.id);
    let wire=serde_json::to_value(&doc).unwrap();
    assert!(wire["records"].get(&key).is_none());assert!(wire["playlistRecords"].get(&key).is_some());
    let parsed:Document=serde_json::from_value(wire).unwrap();parsed.validate().unwrap();
    b.exchange_local(Some(&parsed),None).unwrap();
    assert_eq!(b.db.list_playlists().unwrap()[0].version_ids,vec!["plbv"]);
    // A foreign member remains a stable placeholder until the exact source appears.
    let missing="c".repeat(64);
    let mut journal=protocol::Journal::default();
    journal.doc=parsed.clone();
    journal.edit(key.clone(),json!({"name":"改名","items":[missing.clone(),parsed.selected()[&key]["items"][0]],"sortOrder":0}));
    a.exchange_local(Some(&journal.doc),None).unwrap();
    let stored=a.db.list_playlists().unwrap().remove(0);
    assert_eq!(stored.name,"改名");assert_eq!(stored.version_ids,vec![format!("sync:{missing}"),"plav".into()]);
    let again=a.exchange_local(None,None).unwrap();assert_eq!(again.selected()[&key]["items"],journal.doc.selected()[&key]["items"]);
    a.db.delete_playlist(&p.id).unwrap();let deleted=a.exchange_local(None,None).unwrap();
    b.exchange_local(Some(&deleted),None).unwrap();assert!(b.db.list_playlists().unwrap().is_empty());
    b.exchange_local(Some(&parsed),None).unwrap();assert!(b.db.list_playlists().unwrap().is_empty());
    assert_eq!(b.db.playlist_tracks(Some(&["plbv".into()]),true).unwrap().len(),1);
    assert!(a.db.update_playlist(&p.id,None,&[],None,None,None,None).is_err());
}

#[test]
fn playlist_only_online_source_imports_without_collecting_or_liking() {
    let (_da,a)=fixture("poa");let (_db,b)=fixture("pob");
    a.db.with_conn(|c|{c.execute_batch("INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES('pows','poaw','asmr_one','1234','now'); INSERT INTO media_sources(id,version_id,source_kind,provider,source_work_id,remote_file_id,remote_relative_path,format,byte_size,created_at,updated_at) VALUES('poremote','poav','online_provider','asmr_one','1234','stable','disc/track.wav','wav',123,'now','now');")?;Ok(())}).unwrap();
    let p=a.db.create_playlist("在线歌单",vec!["poav".into()]).unwrap();let doc=a.exchange_local(None,None).unwrap();
    assert!(!doc.online_works.is_empty());
    b.exchange_local(Some(&doc),None).unwrap();b.exchange_local(Some(&doc),None).unwrap();
    let group=b.db.list_playlists().unwrap().into_iter().find(|v|v.id==p.id).unwrap();
    assert!(!group.version_ids[0].starts_with("sync:"));
    let tracks=b.db.playlist_tracks(Some(&group.version_ids),true).unwrap();assert!(tracks[0].available);assert_eq!(tracks[0].title,"track.wav");
    b.db.with_conn(|c|{for table in ["favorites","collected_works","favorite_versions"]{assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get::<_,i64>(0))?,0);}Ok(())}).unwrap();
}

#[test]
fn playlist_verified_file_identity_restores_online_source_without_identity_drift() {
    let (_dir,service)=fixture("verifiedplaylist");
    let work=catalog::hash(b"asmr_one|5678");let track=catalog::hash(b"asmr_one|5678|file");let local=catalog::hash(b"verified file");
    let id="android-playlist";let key=format!("playlist|{}|{id}",catalog::hash(id.as_bytes()));
    let mut j=protocol::Journal::default();j.edit(key.clone(),json!({"name":"跨端歌单","items":[local.clone()],"sortOrder":0}));
    j.doc.online_works.insert(work,protocol::OnlineWork{source_id:"5678".into(),title:"已验证作品".into(),rating:"UNRATED".into(),local_key:None,tracks:std::collections::BTreeMap::from([(track,protocol::OnlineTrack{file_id:"file".into(),path:"audio.wav".into(),bytes:None,local_key:Some(local.clone())})])});
    service.exchange_local(Some(&j.doc),None).unwrap();
    let group=service.db.list_playlists().unwrap().remove(0);assert!(!group.version_ids[0].starts_with("sync:"));
    for _ in 0..2 {let doc=service.exchange_local(Some(&j.doc),None).unwrap();assert_eq!(doc.selected()[&key]["items"],json!([local]));}
    service.db.with_conn(|c|{assert_eq!(c.query_row("SELECT count(*) FROM media_sources WHERE remote_file_id='file'",[],|r|r.get::<_,i64>(0))?,1);Ok(())}).unwrap();
}
#[test]
fn history_only_online_track_exports_resolvable_identity() {
    let (_dir, service) = fixture("a");
    service.db.with_conn(|c|{c.execute_batch("INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES('ws','aw','asmr_one','1234','now');
        INSERT INTO media_sources(id,version_id,source_kind,provider,source_work_id,remote_file_id,remote_relative_path,format,byte_size,created_at,updated_at) VALUES('remote','av','online_provider','asmr_one','1234','stable','disc/track.wav','wav',123,'now','now');
        INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES('av',1234,0,'2026-01-01T00:00:00Z');")?;Ok(())}).unwrap();
    let doc = service.exchange_local(None, None).unwrap();
    let parent = catalog::hash(b"asmr_one|1234");
    let key = catalog::hash(b"asmr_one|1234|stable");
    assert_eq!(
        doc.online_works[&parent].tracks[&key].path,
        "disc/track.wav"
    );
    assert!(!doc.selected().contains_key(&format!("library|{parent}|")));
    let mut verified = doc.online_works.clone();
    let local_hash = catalog::hash(b"alternate file identity");
    verified
        .get_mut(&parent)
        .unwrap()
        .tracks
        .get_mut(&key)
        .unwrap()
        .local_key = Some(local_hash.clone());
    service
        .db
        .set_setting(
            "device_sync_verified_online_works",
            &codec::to_string(&verified).unwrap(),
        )
        .unwrap();
    let next = service.exchange_local(None, None).unwrap();
    assert_eq!(
        next.online_works[&parent].tracks[&key].local_key.as_ref(),
        Some(&local_hash)
    );
    let mut damaged = doc.clone();
    damaged
        .online_works
        .get_mut(&parent)
        .unwrap()
        .tracks
        .get_mut(&key)
        .unwrap()
        .file_id = "different".into();
    assert!(damaged.validate().is_err());
}
#[test]
fn sync_content_matching_tombstones_and_idempotence() {
    let (_a, a) = fixture("a");
    let (_b, b) = fixture("b");
    a.db.with_conn(|c|{c.execute_batch("INSERT INTO favorites VALUES('aw','2026-01-01'); INSERT INTO favorite_versions VALUES('av','2026-01-01'); INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES('av',3500,0,'2026-01-01T00:00:00Z'); INSERT INTO bookmarks(id,version_id,position_ms,note,created_at,updated_at) VALUES('mark','av',2500,'双语笔记 日本語','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');")?;Ok(())}).unwrap();
    let doc = a.exchange_local(None, None).unwrap();
    let merged = b.exchange_local(Some(&doc), None).unwrap();
    b.db.with_conn(|c| {
        assert_eq!(
            c.query_row(
                "SELECT position_ms FROM playback_positions WHERE version_id='bv'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            3500
        );
        assert_eq!(
            c.query_row(
                "SELECT note FROM bookmarks WHERE version_id='bv'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "双语笔记 日本語"
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM favorites WHERE work_id='bw'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        Ok(())
    })
    .unwrap();
    let repeated = b.exchange_local(Some(&doc), None).unwrap();
    assert_eq!(
        codec::to_string(&merged).unwrap(),
        codec::to_string(&repeated).unwrap()
    );
    a.exchange_local(Some(&merged), None).unwrap();
    a.db.with_conn(|c| {
        c.execute("DELETE FROM bookmarks WHERE id='mark'", [])?;
        Ok(())
    })
    .unwrap();
    let deleted = a.exchange_local(None, None).unwrap();
    b.exchange_local(Some(&deleted), None).unwrap();
    b.exchange_local(Some(&doc), None).unwrap();
    b.db.with_conn(|c| {
        assert_eq!(
            c.query_row("SELECT count(*) FROM bookmarks", [], |r| r.get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .unwrap();
}
#[test]
fn online_like_creates_missing_work_once_and_removal_propagates() {
    let (_a, a) = fixture("a");
    let (_b, b) = fixture("b");
    a.db.with_conn(|c| {c.execute_batch("INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES('src','aw','asmr_one','1234','now'); INSERT INTO favorites VALUES('aw','now'); INSERT INTO works(id,title,created_at,updated_at) VALUES('only','仅在音频库','now','now'); INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES('src2','only','asmr_one','5678','now'); INSERT INTO collected_works VALUES('only','now');")?;Ok(())}).unwrap();
    let doc = a.exchange_local(None, None).unwrap();
    assert_eq!(doc.online_works.len(), 2);
    let key = catalog::hash(b"asmr_one|1234");
    let mut rated = doc.clone();
    rated.online_works.get_mut(&key).unwrap().rating = "NSFW".into();
    a.exchange_local(Some(&rated), None).unwrap();
    // A desktop entry without a classification must not undo the peer's known rating.
    assert_eq!(
        a.exchange_local(None, None).unwrap().online_works[&key].rating,
        "NSFW"
    );
    let wire = codec::to_value(&doc).unwrap();
    assert_eq!(wire["libraryRecords"].as_object().unwrap().len(), 1);
    assert!(!wire["records"]
        .as_object()
        .unwrap()
        .keys()
        .any(|k| k.starts_with("library|")));
    let received = codec::from_value(wire).unwrap();
    for _ in 0..2 {
        b.exchange_local(Some(&received), None).unwrap();
    }
    b.db.with_conn(|c|{
        assert_eq!(c.query_row("SELECT count(*) FROM collected_works JOIN work_sources USING(work_id) WHERE source_work_id='5678'",[],|r|r.get::<_,i64>(0))?,1);
        assert_eq!(c.query_row("SELECT count(*) FROM favorites JOIN work_sources USING(work_id) WHERE source_work_id='5678'",[],|r|r.get::<_,i64>(0))?,0);
        Ok(())
    }).unwrap();
    let count = || {
        b.db.with_conn(|c|Ok(c.query_row("SELECT count(*) FROM favorites JOIN work_sources USING(work_id) WHERE source_work_id='1234'",[],|r|r.get::<_,i64>(0))?)).unwrap()
    };
    assert_eq!(count(), 1);
    a.db.with_conn(|c| {
        c.execute("DELETE FROM favorites WHERE work_id='aw'", [])?;
        Ok(())
    })
    .unwrap();
    b.exchange_local(Some(&a.exchange_local(None, None).unwrap()), None)
        .unwrap();
    assert_eq!(count(), 0);
}
#[test]
fn sync_stale_conflict_choice_cannot_overwrite_a_new_local_edit() {
    let (_dir, service) = fixture("stale");
    service
        .db
        .with_conn(|c| {
            c.execute("INSERT INTO favorites VALUES('stalew','2026-01-01')", [])?;
            Ok(())
        })
        .unwrap();
    let doc = service.exchange_local(None, None).unwrap();
    let (key, rows) = doc
        .records
        .iter()
        .find(|(key, _)| key.starts_with("work|"))
        .unwrap();
    let chosen = &rows[0];
    service
        .db
        .with_conn(|c| {
            c.execute("DELETE FROM favorites", [])?;
            Ok(())
        })
        .unwrap();
    assert!(service
        .exchange_local(None, Some((key, &chosen.device, chosen.seq)))
        .is_err());
    let next = service.exchange_local(None, None).unwrap();
    assert_eq!(next.selected()[key], false);
}


#[test]
fn remote_resume_survives_restart_dismisses_and_never_repeats_unchanged_progress() {
    let (_a,a)=fixture("resumea");let (dir,b)=fixture("resumeb");
    a.db.with_conn(|c|{c.execute("INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES('resumeav',4200,0,'2026-11-11T01:00:00Z')",[])?;Ok(())}).unwrap();
    let remote=a.exchange_local(None,None).unwrap();
    b.exchange_local(Some(&remote),None).unwrap();
    let candidate=b.resume_candidate(None,None).unwrap().unwrap();
    assert_eq!(candidate["versionId"],"resumebv");assert_eq!(candidate["positionMs"],4200);
    assert_eq!(candidate["deviceName"],"Windows 电脑");
    let reopened=SyncService::new(Database::open_and_migrate(&dir.path().join("data"),&dir.path().join("cache")).unwrap());
    assert_eq!(reopened.resume_candidate(None,None).unwrap().unwrap()["id"],candidate["id"]);
    reopened.dismiss_resume(None,candidate["id"].as_str().unwrap()).unwrap();
    reopened.exchange_local(Some(&remote),None).unwrap();
    assert!(reopened.resume_candidate(None,None).unwrap().is_none());
    let details:Value=serde_json::from_str(&reopened.db.get_setting(&ProviderId::Github.setting("device_sync_details")).unwrap().unwrap()).unwrap();
    let position=details["summary"].as_array().unwrap().iter().find(|r|r["kind"]=="position").unwrap();
    assert_eq!(position["received"],0);assert_eq!(position["total"],1);
}

#[test]
fn local_listening_invalidates_handoff_and_old_button_cannot_accept_new_offer() {
    let (_a,a)=fixture("la");let (_b,b)=fixture("lb");
    a.db.with_conn(|c|{c.execute("INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES('lav',1200,0,'2026-11-11T01:00:00Z')",[])?;Ok(())}).unwrap();
    b.exchange_local(Some(&a.exchange_local(None,None).unwrap()),None).unwrap();
    let candidate=b.resume_candidate(None,None).unwrap().unwrap();
    assert!(b.resume_candidate(None,Some("outdated-id")).is_err());
    b.db.with_conn(|c|{c.execute("UPDATE playback_positions SET position_ms=2200,updated_at='2026-11-11T01:01:00Z' WHERE version_id='lbv'",[])?;Ok(())}).unwrap();
    assert!(b.resume_candidate(None,Some(candidate["id"].as_str().unwrap())).unwrap().is_none());
}

#[test]
fn conflicted_completed_or_unmatched_positions_do_not_offer_resume() {
    let mut doc=Document::default();
    let key=format!("position|{}|","a".repeat(64));
    let row=protocol::Revision{device:"other".into(),seq:1,clock:1,context:Default::default(),value:json!({"positionMs":2000,"durationMs":10000,"updatedMs":1000,"completed":false})};
    doc.records.insert(key.clone(),vec![row.clone()]);
    let mut catalog=catalog::Catalog{versions:Default::default(),works:Default::default(),names:Default::default()};
    let (details,resume)=insights::describe(&doc,&Default::default(),&catalog,"local");
    assert!(resume.is_none());assert_eq!(details["pendingItems"].as_array().unwrap().len(),1);
    catalog.versions.insert("v".into(),"a".repeat(64));
    doc.records.get_mut(&key).unwrap().push(protocol::Revision{device:"third".into(),..row});
    assert!(insights::describe(&doc,&Default::default(),&catalog,"local").1.is_none());
    doc.records.get_mut(&key).unwrap().truncate(1);
    doc.records.get_mut(&key).unwrap()[0].value["completed"]=json!(true);
    assert!(insights::describe(&doc,&Default::default(),&catalog,"local").1.is_none());
}
