//! Builds requests for every API method and checks method, path and query.

use yutu_api::generated::METHODS;
use yutu_api::schema::Location;
use yutu_api::{Media, Service, Value};

fn svc() -> Service {
    Service::with_base("https://youtube.googleapis.com/", None)
}

#[test]
fn list_videos_matches_go_query() {
    let s = svc();
    let call = s
        .call("youtube.videos.list")
        .params("part", &["id".into(), "snippet".into()])
        .params("id", &["a".into(), "b".into()])
        .param("maxResults", 5);
    assert_eq!(
        call.preview(),
        ("GET", "https://youtube.googleapis.com/youtube/v3/videos?alt=json&id=a&id=b&maxResults=5&part=id&part=snippet&prettyPrint=false".to_string())
    );
}

#[test]
fn caption_download_fills_the_path() {
    let s = svc();
    let call = s
        .call("youtube.captions.download")
        .param("id", "cap 1")
        .param("tfmt", "srt");
    assert_eq!(call.preview(), ("GET", "https://youtube.googleapis.com/youtube/v3/captions/cap%201?alt=json&prettyPrint=false&tfmt=srt".to_string()));
}

#[test]
fn uploads_go_to_the_upload_path_as_multipart() {
    let s = svc();
    let call = s
        .call("youtube.videos.insert")
        .param("part", "snippet,status")
        .media(Media::Bytes(vec![1, 2, 3]));
    assert_eq!(
        call.preview(),
        ("POST", "https://youtube.googleapis.com/upload/youtube/v3/videos?alt=json&part=snippet%2Cstatus&prettyPrint=false&uploadType=multipart".to_string())
    );
}

#[test]
fn bodies_end_with_a_newline_like_json_encoder() {
    let s = svc();
    let mut p = Value::new_struct(yutu_api::generated::PLAYLIST);
    p.ensure("snippet").set_str("title", "T");
    let call = s.call("youtube.playlists.insert").body(&p);
    assert_eq!(call.body_text(), Some("{\"snippet\":{\"title\":\"T\"}}\n"));
}

#[test]
fn every_method_places_every_parameter() {
    let s = svc();
    assert_eq!(METHODS.len(), 83);
    for m in METHODS {
        let mut call = s.call(m.id);
        for p in m.params {
            call = call.param(p.name, format!("v-{}", p.name));
        }
        let (verb, url) = call.preview();
        assert_eq!(verb, m.http_method, "{}", m.id);
        let (path, query) = url
            .trim_start_matches("https://youtube.googleapis.com/")
            .split_once('?')
            .unwrap();
        assert!(!path.contains('{'), "{}: unfilled path {path}", m.id);
        for p in m.params {
            let encoded = format!("v-{}", p.name).replace('.', ".");
            match p.location {
                Location::Path => assert!(path.ends_with(&encoded), "{}: {path}", m.id),
                Location::Query => assert!(
                    query
                        .split('&')
                        .any(|kv| kv == format!("{}={encoded}", p.name).replace('$', "%24")),
                    "{}: {} missing in {query}",
                    m.id,
                    p.name
                ),
            }
        }
        assert!(
            query.contains("alt=json") && query.contains("prettyPrint=false"),
            "{}",
            m.id
        );
    }
}
