//! Offline tests for channel filtering, digests, version ordering, headers and
//! the swap script. No test opens a socket.

use super::*;
use crate::json;
use crate::net;

fn asset(name: &str, url: &str) -> Asset {
    Asset {
        name: name.to_owned(),
        url: url.to_owned(),
        digest: None,
        size: 0,
    }
}

fn release(tag: &str, pre: bool) -> Release {
    Release {
        tag: tag.to_owned(),
        version: Version::parse(tag).unwrap(),
        notes: String::new(),
        exe: asset("a.exe", "u"),
        sha256: None,
        prerelease: pre,
    }
}

#[test]
fn versions_parse_and_compare() {
    let rel = |s: &str| Version::parse(s).unwrap();
    assert!(rel("v1.0.1").is_newer_than(&rel("1.0.0")));
    // A stable release beats a dev build of the same version.
    assert!(rel("1.0.0").is_newer_than(&rel("1.0.0-65-abc1234")));
    assert!(rel("1.0.0-66-abc1234").is_newer_than(&rel("1.0.0-65-abc1234")));
    assert!(!rel("1.0.0-65-abc1234").is_newer_than(&rel("1.0.0")));
    assert_eq!(rel("1.0.0-65-abc1234").build, Some(65));
    assert_eq!(rel("v1.0.0").build, None);
}

#[test]
fn semver_prerelease_identifiers_order_correctly() {
    let rel = |s: &str| Version::parse(s).unwrap();
    assert!(rel("1.0.0").is_newer_than(&rel("1.0.0-beta.11")));
    assert!(rel("1.0.0-beta.11").is_newer_than(&rel("1.0.0-beta.2")));
    assert!(rel("1.0.0-beta").is_newer_than(&rel("1.0.0-alpha")));
    assert_eq!(rel("1.2.0-beta.1").pre, vec!["beta", "1"]);
}

/// The `/releases` list feeds both channels; drafts, entries without assets
/// and older versions must all be skipped.
#[test]
fn channel_picks_skip_drafts_and_broken_entries() {
    let list = json::parse(
        r#"[
          {"tag_name":"v9.9.9","draft":true,"prerelease":false,
           "assets":[{"name":"a.exe","browser_download_url":"u"},{"name":"a.exe.sha256","browser_download_url":"v"}]},
          {"tag_name":"v2.0.0","draft":false,"prerelease":true,
           "assets":[{"name":"b.exe","browser_download_url":"u"},{"name":"b.exe.sha256","browser_download_url":"v"}]},
          {"tag_name":"v1.0.0","draft":false,"prerelease":false,
           "assets":[{"name":"c.exe","browser_download_url":"u"},{"name":"c.exe.sha256","browser_download_url":"v"}]},
          {"tag_name":"v3.0.0","draft":false,"prerelease":false,"assets":[]}
        ]"#,
    )
    .expect("parses");
    let arr = list.as_arr().expect("array");
    let beta = release::pick(arr, Channel::Beta).expect("beta winner");
    assert_eq!(beta.version.text, "2.0.0", "草稿与缺资产的都要跳过");
    assert!(beta.prerelease, "选中的是预发布，界面要说清楚");

    let stable = release::pick(arr, Channel::Stable).expect("stable winner");
    assert_eq!(stable.version.text, "1.0.0", "稳定通道不收预发布");
    assert!(!stable.prerelease);
}

/// Real payload: the rolling pre-release tag is `dev`, so the version comes
/// from the title and the asset name.
#[test]
fn version_is_read_from_title_or_asset_when_tag_is_a_channel_name() {
    let doc = json::parse(
        r#"{"tag_name":"dev","name":"Development build 2.0.0-8-00cb41d",
            "draft":false,"prerelease":true,
            "assets":[{"name":"NookSill-2.0.0-8-00cb41d-win64-debug.exe","browser_download_url":"u"},
                      {"name":"NookSill-2.0.0-8-00cb41d-win64-debug.exe.sha256","browser_download_url":"v"}]}"#,
    )
    .expect("parses");
    let parsed = release::parse_release(&doc).expect("dev release parses");
    assert_eq!(parsed.version.major, 2);
    assert_eq!(parsed.version.build, Some(8));
    assert_eq!(parsed.version.text, "2.0.0-8", "显示用的文本要干净");
    assert!(parsed.prerelease);
    assert!(parsed.sha256.is_some());
}

#[test]
fn normal_tag_still_wins() {
    let doc = json::parse(
        r#"{"tag_name":"v1.2.3","name":"NookSill 1.2.3","draft":false,"prerelease":false,
            "assets":[{"name":"NookSill-1.2.3-win64.exe","browser_download_url":"u"},
                      {"name":"NookSill-1.2.3-win64.exe.sha256","browser_download_url":"v"}]}"#,
    )
    .expect("parses");
    let parsed = release::parse_release(&doc).expect("release parses");
    assert_eq!(parsed.version.text, "1.2.3");
    assert_eq!(parsed.version.build, None);
}

#[test]
fn digest_and_upload_state_are_honoured() {
    let doc = json::parse(
        r#"{"tag_name":"v2.0.1","draft":false,"prerelease":false,
            "assets":[
              {"name":"half.exe","browser_download_url":"u","state":"uploading","size":10},
              {"name":"full.exe","browser_download_url":"u","state":"uploaded","size":123,
               "digest":"sha256:3F4BCE7EBED48898AB1D60F9B31B88623D4D09EF95FE193BBD727B6E9F7714CB"}]}"#,
    )
    .expect("parses");
    let parsed = release::parse_release(&doc).expect("parses");
    assert_eq!(parsed.exe.name, "full.exe", "半成品资产不能入选");
    assert_eq!(parsed.exe.size, 123);
    assert_eq!(
        parsed.exe.digest.as_deref(),
        Some("3f4bce7ebed48898ab1d60f9b31b88623d4d09ef95fe193bbd727b6e9f7714cb")
    );
    assert_eq!(
        release::digest_of(Some("sha256:abc")),
        None,
        "短摘要必须拒绝"
    );
}

#[test]
fn beta_channel_prefers_the_dev_build_over_a_same_triple_stable_release() {
    let list = json::parse(
        r#"[
          {"tag_name":"v2.0.0","draft":false,"prerelease":false,
           "assets":[{"name":"NookSill-2.0.0-win64.exe","browser_download_url":"u"}]},
          {"tag_name":"dev","name":"Development build 2.0.0-12-abc1234","draft":false,"prerelease":true,
           "assets":[{"name":"NookSill-2.0.0-12-abc1234-win64-debug.exe","browser_download_url":"u"}]}
        ]"#,
    )
    .expect("parses");
    let arr = list.as_arr().expect("array");
    let beta = release::pick(arr, Channel::Beta).expect("beta winner");
    assert!(beta.prerelease, "测试通道必须拿到 dev 构建");
    assert_eq!(beta.version.build, Some(12));
    let stable = release::pick(arr, Channel::Stable).expect("stable winner");
    assert!(!stable.prerelease);
}

#[test]
fn empty_release_list_reports_no_release() {
    assert!(release::pick(&[], Channel::Stable).is_none());
    assert!(release::pick(&[], Channel::Beta).is_none());
}

#[test]
fn channel_is_explicit_not_inferred_from_the_install() {
    let stable = Version::parse("2.0.0").unwrap();
    let dev = Version::parse("2.0.0-9-abc1234").unwrap();
    let beta_release = release("2.0.1-9-abc1234", true);
    let same = release("2.0.0", false);
    let newer = release("2.0.1", false);

    assert!(!is_offered(&same, &stable, Channel::Stable), "同名不算更新");
    assert!(is_offered(&newer, &stable, Channel::Stable));
    assert!(
        !is_offered(&beta_release, &stable, Channel::Stable),
        "稳定通道不收预发布"
    );
    assert!(
        is_offered(&beta_release, &stable, Channel::Beta),
        "测试通道收预发布"
    );
    assert!(
        !is_offered(&beta_release, &dev, Channel::Beta),
        "同一构建号不重复提供"
    );
    assert!(is_offered(
        &release("2.0.1-10-abc1234", true),
        &dev,
        Channel::Beta
    ));
    assert!(is_offered(&newer, &dev, Channel::Beta));

    // Switching a stable install into the beta channel must offer the dev
    // build even though both carry the same 2.0.0 triple.
    let installed_stable = Version::parse("2.0.0").unwrap();
    let dev_cut = release("2.0.0-12-abc1234", true);
    assert!(!is_offered(&dev_cut, &installed_stable, Channel::Stable));
    assert!(is_offered(&dev_cut, &installed_stable, Channel::Beta));
}

#[test]
fn rate_limit_error_is_readable() {
    let e = UpdateError::Net(net::NetError::Status(403));
    assert!(e.to_string().contains("限流"), "{e}");
    let res = net::Response {
        status: 403,
        body: Vec::new(),
        bytes: 0,
        headers: vec![
            ("x-ratelimit-reset".to_owned(), "0".to_owned()),
            ("retry-after".to_owned(), "37".to_owned()),
        ],
    };
    assert!(http::rate_limited(&res).to_string().contains("37"));
}

#[test]
fn conditional_request_headers_are_stable() {
    let plain = http::headers(None);
    assert!(plain.contains("User-Agent:"));
    assert!(plain.contains("X-GitHub-Api-Version: 2022-11-28"));
    assert!(!plain.contains("If-None-Match"));

    let tagged = http::headers(Some("W/\"abc\""));
    assert!(
        tagged.contains("If-None-Match: W/\"abc\""),
        "ETag 必须原样回传"
    );
}

#[test]
fn mirror_prefix_is_a_plain_concat() {
    assert_eq!(
        mirror_url("https://gh-proxy.com/", "https://github.com/a/b.exe"),
        "https://gh-proxy.com/https://github.com/a/b.exe"
    );
    assert_eq!(
        mirror_url("", "https://github.com/a/b.exe"),
        "https://github.com/a/b.exe"
    );
}

#[test]
fn checksum_file_is_parsed() {
    let text = "3f4bce7ebed48898ab1d60f9b31b88623d4d09ef95fe193bbd727b6e9f7714cb  NookSill.exe\n";
    assert_eq!(
        parse_sha256_file(text).as_deref(),
        Some("3f4bce7ebed48898ab1d60f9b31b88623d4d09ef95fe193bbd727b6e9f7714cb")
    );
    assert!(parse_sha256_file("not a hash").is_none());
}

#[test]
fn sha256_matches_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn silent_swap_never_restarts_the_process() {
    let current = std::path::Path::new(r"C:\app\nooksill-dock.exe");
    let staged = std::path::Path::new(r"C:\app\NookSill.update.exe");
    let silent = stage::script_body(current, staged, false);
    assert!(silent.contains("move /Y"));
    assert!(!silent.contains("start \"\""), "静默替换不能重启应用");
    let restart = stage::script_body(current, staged, true);
    assert!(restart.contains("start \"\""), "手动更新要重启应用");
}

#[test]
fn tampered_staged_file_is_rejected() {
    let staged = std::env::temp_dir().join("nooksill-staged-test.exe");
    std::fs::write(&staged, b"hello").unwrap();
    let digest = sha256_hex(b"hello");
    stage::write_marker(&digest, 5).unwrap();
    assert!(
        stage::verify_staged(&staged).is_ok(),
        "校验通过的文件可安装"
    );

    std::fs::write(&staged, b"tampered").unwrap();
    assert!(
        stage::verify_staged(&staged).is_err(),
        "字节被替换后必须拒绝安装"
    );
    let marker = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("nooksill-dock.exe"))
        .with_file_name("NookSill.update.marker");
    let _ = std::fs::remove_file(marker);
    let _ = std::fs::remove_file(staged);
}
