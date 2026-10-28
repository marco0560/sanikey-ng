//! Errori di parsing e riferimenti tra i metadati curati.

use super::common::{TestDirectory, error_message, metadata};

#[test]
fn load_curated_metadata_rejects_malformed_items() {
    let directory = TestDirectory::new();
    directory.write("problems.toml", "[[problem]]\nid='missing-title'\n");

    let error = error_message(directory.path());

    assert!(error.contains("title"));
}

#[test]
fn load_curated_metadata_rejects_invalid_toml() {
    let directory = TestDirectory::new();
    directory.write("therapies.toml", "[[therapy]\nid='broken'\n");

    let error = error_message(directory.path());

    assert!(error.contains("TOML non valido"));
}

#[test]
fn load_curated_metadata_rejects_a_file_instead_of_directory() {
    let directory = TestDirectory::new();
    directory.write("not-a-directory", "metadata fixture");

    let error = error_message(&directory.path().join("not-a-directory"));

    assert!(error.contains("il percorso dei metadati non è una directory"));
}

#[test]
fn load_curated_metadata_generates_stable_therapy_ids() {
    let directory = TestDirectory::new();
    directory.write(
        "medications.toml",
        "[[medication]]\nid='atenololo'\nname='Atenololo'\n",
    );
    directory.write(
        "therapies.toml",
        "[[therapy]]\nmedication_id='atenololo'\nrole='antipertensivo'\n\n[[therapy]]\nmedication_id='atenololo'\nrole='cardioprotezione'\n",
    );

    let loaded = metadata(directory.path());

    assert_eq!(
        loaded
            .therapies
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["therapy-atenololo-1", "therapy-atenololo-2"]
    );
    assert_eq!(loaded.therapies[0].role.as_deref(), Some("antipertensivo"));
}

#[test]
fn load_curated_metadata_rejects_duplicate_ids() {
    let directory = TestDirectory::new();
    directory.write(
        "medications.toml",
        "[[medication]]\nid='drug-a'\nname='Drug A'\n",
    );
    directory.write(
        "therapies.toml",
        "[[therapy]]\nid='duplicate'\nmedication_id='drug-a'\n\n[[therapy]]\nid='duplicate'\nmedication_id='drug-a'\n",
    );

    let error = error_message(directory.path());

    assert!(error.contains("id therapy duplicato: duplicate"));
}

#[test]
fn load_curated_metadata_rejects_unknown_medication_reference() {
    let directory = TestDirectory::new();
    directory.write("therapies.toml", "[[therapy]]\nmedication_id='missing'\n");

    let error = error_message(directory.path());

    assert!(error.contains("medication_id sconosciuto missing"));
}

#[test]
fn load_curated_metadata_rejects_aifa_and_non_aifa_overlap() {
    let directory = TestDirectory::new();
    directory.write(
        "medications.toml",
        "[[medication]]\nid='drug-a'\nname='Drug A'\n",
    );
    directory.write(
        "medication_leaflets.toml",
        "[[leaflet]]\nmedication_id='drug-a'\ncodice_sis='123'\naic6='456'\n\n[[unavailable]]\nmedication_id='drug-a'\n",
    );

    let error = error_message(directory.path());

    assert!(error.contains("riferimento AIFA e stato non_aifa"));
}

#[test]
fn load_curated_metadata_rejects_point_with_unknown_series() {
    let directory = TestDirectory::new();
    directory.write(
        "observations/points.toml",
        "[[point]]\nid='point-a'\nseries_id='missing'\nobservation_date='2026-01-01'\nsource_type='manual'\nsource_reference='journal'\n",
    );

    let error = error_message(directory.path());

    assert!(error.contains("series_id sconosciuto missing"));
}
