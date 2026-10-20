//! Against the network (`cargo test -p shellwing-update -- --ignored`): the
//! HivePaaS CLI's list of its releases is signed with the same keys, under
//! its own context. It opens under that one, and never as shellwing's.

const CLI_LIST: &str =
    "https://raw.githubusercontent.com/hivepaas/hivepaas-cli/release/release.signed.json";

#[tokio::test]
#[ignore = "reads from GitHub"]
async fn a_list_signed_with_the_release_keys_opens_under_its_own_context_only() {
    let list = shellwing_update::fetch(CLI_LIST).await.expect("fetch");
    let keys = shellwing_update::trusted_keys().expect("keys");
    let data = shellwing_update::envelope::open(&keys, &list, "hivepaas-cli-release-v1")
        .expect("the CLI's list opens under its context");
    assert!(shellwing_update::Manifest::parse(&data).is_ok());
    assert!(shellwing_update::offer(&list, &shellwing_update::current()).is_err());
}
