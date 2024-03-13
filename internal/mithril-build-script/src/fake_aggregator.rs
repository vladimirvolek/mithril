use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub type ArtifactId = String;
pub type FileContent = String;

/// In memory representation of a folder containing data imported using the `scripts/import.sh` script
/// of the fake aggregator.
#[derive(Debug, Default)]
pub struct FakeAggregatorData {
    epoch_settings: FileContent,

    certificates_list: FileContent,
    individual_certificates: BTreeMap<ArtifactId, FileContent>,

    snapshots_list: FileContent,
    individual_snapshots: BTreeMap<ArtifactId, FileContent>,

    msds_list: FileContent,
    individual_msds: BTreeMap<ArtifactId, FileContent>,

    ctx_snapshots_list: FileContent,
    individual_ctx_snapshots: BTreeMap<ArtifactId, FileContent>,
    ctx_proofs: BTreeMap<ArtifactId, FileContent>,
}

impl FakeAggregatorData {
    pub fn load_from_folder(folder: &Path) -> Self {
        fn extract_artifact_id(filename: &str, prefix: &str) -> String {
            let id_with_extension = filename.strip_prefix(prefix).unwrap();
            id_with_extension.strip_suffix(".json").unwrap().to_string()
        }

        let mut data = FakeAggregatorData::default();

        for entry in list_json_files_in_folder(folder) {
            let filename = entry.file_name().to_string_lossy().to_string();
            let file_content = fs::read_to_string(&entry.path()).unwrap_or_else(|_| {
                panic!(
                    "Could not read file content, file_path: {}",
                    entry.path().display()
                )
            });

            match filename.as_str() {
                "epoch-settings.json" => {
                    data.epoch_settings = file_content;
                }
                "mithril-stake-distributions.json" => {
                    data.msds_list = file_content;
                }
                "snapshots.json" => {
                    data.snapshots_list = file_content;
                }
                "certificates.json" => {
                    data.certificates_list = file_content;
                }
                "ctx-snapshots.json" => {
                    data.ctx_snapshots_list = file_content;
                }
                _ if filename.starts_with("mithril-stake-distribution-") => {
                    data.individual_msds.insert(
                        extract_artifact_id(&filename, "mithril-stake-distribution-"),
                        file_content,
                    );
                }
                _ if filename.starts_with("snapshot-") => {
                    data.individual_snapshots
                        .insert(extract_artifact_id(&filename, "snapshot-"), file_content);
                }
                _ if filename.starts_with("certificate-") => {
                    data.individual_certificates
                        .insert(extract_artifact_id(&filename, "certificate-"), file_content);
                }
                _ if filename.starts_with("ctx-snapshot-") => {
                    data.individual_ctx_snapshots.insert(
                        extract_artifact_id(&filename, "ctx-snapshot-"),
                        file_content,
                    );
                }
                _ if filename.starts_with("ctx-proof-") => {
                    data.ctx_proofs
                        .insert(extract_artifact_id(&filename, "ctx-proof-"), file_content);
                }
                // unknown file
                _ => {}
            }
        }

        data
    }

    pub fn generate_code_for_ids(self) -> String {
        Self::assemble_code(
            &[
                generate_ids_array(
                    "snapshot_digests",
                    BTreeSet::from_iter(self.individual_snapshots.keys().cloned()),
                ),
                generate_ids_array(
                    "msd_hashes",
                    BTreeSet::from_iter(self.individual_msds.keys().cloned()),
                ),
                generate_ids_array(
                    "certificate_hashes",
                    BTreeSet::from_iter(self.individual_certificates.keys().cloned()),
                ),
                generate_ids_array(
                    "ctx_snapshot_hashes",
                    BTreeSet::from_iter(self.individual_ctx_snapshots.keys().cloned()),
                ),
                generate_ids_array(
                    "proof_transaction_hashes",
                    BTreeSet::from_iter(self.ctx_proofs.keys().cloned()),
                ),
            ],
            false,
        )
    }

    pub fn generate_code_for_all_data(self) -> String {
        Self::assemble_code(
            &[
                generate_list_getter("epoch_settings", self.epoch_settings),
                generate_ids_array(
                    "snapshot_digests",
                    BTreeSet::from_iter(self.individual_snapshots.keys().cloned()),
                ),
                generate_artifact_getter("snapshots", self.individual_snapshots),
                generate_list_getter("snapshot_list", self.snapshots_list),
                generate_ids_array(
                    "msd_hashes",
                    BTreeSet::from_iter(self.individual_msds.keys().cloned()),
                ),
                generate_artifact_getter("msds", self.individual_msds),
                generate_list_getter("msd_list", self.msds_list),
                generate_ids_array(
                    "certificate_hashes",
                    BTreeSet::from_iter(self.individual_certificates.keys().cloned()),
                ),
                generate_artifact_getter("certificates", self.individual_certificates),
                generate_list_getter("certificate_list", self.certificates_list),
                generate_ids_array(
                    "ctx_snapshot_hashes",
                    BTreeSet::from_iter(self.individual_ctx_snapshots.keys().cloned()),
                ),
                generate_artifact_getter("ctx_snapshots", self.individual_ctx_snapshots),
                generate_list_getter("ctx_snapshots_list", self.ctx_snapshots_list),
                generate_ids_array(
                    "proof_transaction_hashes",
                    BTreeSet::from_iter(self.ctx_proofs.keys().cloned()),
                ),
                generate_artifact_getter("ctx_proofs", self.ctx_proofs),
            ],
            true,
        )
    }

    fn assemble_code(functions_code: &[String], include_use_btree_map: bool) -> String {
        format!(
            "{}{}
",
            if include_use_btree_map {
                "use std::collections::BTreeMap;

"
            } else {
                ""
            },
            functions_code.join(
                "

"
            )
        )
    }
}

pub fn list_json_files_in_folder(folder: &Path) -> impl Iterator<Item = std::fs::DirEntry> + '_ {
    crate::list_files_in_folder(folder)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
}

// pub(crate) fn $fun_name()() -> BTreeMap<String, String>
pub fn generate_artifact_getter(
    fun_name: &str,
    source_jsons: BTreeMap<ArtifactId, FileContent>,
) -> String {
    let mut artifacts_list = String::new();

    for (artifact_id, file_content) in source_jsons {
        write!(
            artifacts_list,
            r###"
        (
            "{}",
            r#"{}"#
        ),"###,
            artifact_id, file_content
        )
        .unwrap();
    }

    format!(
        r###"pub(crate) fn {}() -> BTreeMap<String, String> {{
    [{}
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}}"###,
        fun_name, artifacts_list
    )
}

/// pub(crate) fn $fun_name() -> &'static str
pub fn generate_list_getter(fun_name: &str, source_json: FileContent) -> String {
    format!(
        r###"pub(crate) fn {}() -> &'static str {{
    r#"{}"#
}}"###,
        fun_name, source_json
    )
}

/// pub(crate) fn $array_name() -> [&'a str; $ids.len]
pub fn generate_ids_array(array_name: &str, ids: BTreeSet<ArtifactId>) -> String {
    let mut ids_list = String::new();

    for id in &ids {
        write!(
            ids_list,
            r#"
        "{}","#,
            id
        )
        .unwrap();
    }

    format!(
        r###"pub(crate) const fn {}<'a>() -> [&'a str; {}] {{
    [{}
    ]
}}"###,
        array_name,
        ids.len(),
        ids_list,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_ids_array_with_empty_data() {
        assert_eq!(
            generate_ids_array("snapshots_digests", BTreeSet::new()),
            "pub(crate) const fn snapshots_digests<'a>() -> [&'a str; 0] {
    [
    ]
}"
        );
    }

    #[test]
    fn generate_ids_array_with_non_empty_data() {
        assert_eq!(
            generate_ids_array(
                "snapshots_digests",
                BTreeSet::from_iter(["abc".to_string(), "def".to_string(), "hij".to_string()])
            ),
            r#"pub(crate) const fn snapshots_digests<'a>() -> [&'a str; 3] {
    [
        "abc",
        "def",
        "hij",
    ]
}"#
        );
    }

    #[test]
    fn assemble_code_with_btree_use() {
        assert_eq!(
            "use std::collections::BTreeMap;

fn a() {}

fn b() {}
",
            FakeAggregatorData::assemble_code(
                &["fn a() {}".to_string(), "fn b() {}".to_string()],
                true
            )
        )
    }

    #[test]
    fn assemble_code_without_btree_use() {
        assert_eq!(
            "fn a() {}

fn b() {}
",
            FakeAggregatorData::assemble_code(
                &["fn a() {}".to_string(), "fn b() {}".to_string()],
                false
            )
        )
    }
}