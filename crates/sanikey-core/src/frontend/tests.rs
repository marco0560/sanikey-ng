use super::{
    APP_JS, CHART_JS, CHART_LICENSE, CHART_NAVIGATION_JS, EXTENDED_CHART_TEMPLATE, INDEX_TEMPLATE,
    MATERIAL_WEB_CSS, MATERIAL_WEB_JS, PARAMETER_CHART_JS, SANKEY_LOGO, STYLE_CSS, UI_HELPER_JS,
    build_frontend,
};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::var_os("SANIKEY_TEST_TEMP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = root.join(format!(
            "sanikey-ng-frontend-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "patient-a".to_owned(),
        display_name: "Paziente sintetico".to_owned(),
        source_documents: root.join("source"),
        metadata_directory: root.join("metadata"),
        local_build: root.join("build"),
        usb_uuid: "fixture-volume".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

#[test]
fn frontend_generator_writes_offline_pages_assets_and_vendor_license() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    person.display_name = "<Paziente> & \"sintetico\"".to_owned();
    person.ui.subtitle = "Con <cura> & attenzione".to_owned();
    person.ui.default_tab = "timeline".to_owned();

    let result = build_frontend(&person).expect("frontend sintetico");

    assert!(result.index.is_file());
    assert!(result.extended_chart.is_file());
    assert!(result.script.is_file());
    assert!(result.stylesheet.is_file());
    assert!(result.helper.is_file());
    assert!(result.material_script.is_file());
    assert!(result.material_stylesheet.is_file());
    assert!(result.chart_script.is_file());
    assert!(result.chart_license.is_file());
    assert!(result.web_dir.join("parameter-chart.js").is_file());
    assert!(result.web_dir.join("usb-info.js").is_file());
    assert!(result.web_dir.join("assets/chart-navigation.js").is_file());
    assert!(
        result
            .web_dir
            .join("assets/sanikey-logo-horizontal-transparent.svg")
            .is_file()
    );

    let index = fs::read_to_string(&result.index).expect("index HTML");
    assert!(
        index.contains("<title>SaniKey - &lt;Paziente&gt; &amp; &quot;sintetico&quot;</title>")
    );
    assert!(index.contains("<h1>&lt;Paziente&gt; &amp; &quot;sintetico&quot;</h1>"));
    assert!(index.contains("Con &lt;cura&gt; &amp; attenzione"));
    assert!(index.contains("data-default-section=\"timeline\""));
    let extended = fs::read_to_string(&result.extended_chart).expect("pagina grafici");
    for page in [&index, &extended] {
        assert!(page.find("chart.umd.min.js") < page.find("chart-navigation.js"));
        assert!(!page.contains("type=\"module\""));
    }

    let generated = [
        index,
        fs::read_to_string(&result.script).expect("JavaScript app"),
        fs::read_to_string(&result.helper).expect("JavaScript UI"),
        fs::read_to_string(&result.stylesheet).expect("CSS frontend"),
        fs::read_to_string(&result.material_script).expect("JavaScript Material"),
        fs::read_to_string(&result.material_stylesheet).expect("CSS Material"),
    ]
    .join("\n")
    .replace("https://github.com/marco0560/sanikey", "");
    for forbidden in [
        "telemetry",
        "document.cookie",
        "localstorage",
        "sessionstorage",
        "indexeddb",
        "http://",
        "https://",
        "fetch(",
    ] {
        assert!(!generated.to_ascii_lowercase().contains(forbidden));
    }
    assert!(generated.contains("<script src=\"data.js\"></script>"));
    assert!(generated.contains("<script src=\"assets/chart.umd.min.js\"></script>"));
    assert!(
        fs::read_to_string(&result.chart_license)
            .expect("licenza Chart.js")
            .starts_with("The MIT License (MIT)")
    );
}

#[test]
fn frontend_generator_escapes_dynamic_values_without_reprocessing_them() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    person.display_name = "__SANIKEY_SUBTITLE__ <Ada>".to_owned();
    person.ui.subtitle = "__SANIKEY_DEFAULT_TAB__ & co".to_owned();

    let result = build_frontend(&person).expect("frontend sintetico");
    let index = fs::read_to_string(&result.index).expect("index HTML");

    assert!(index.contains("<h1>__SANIKEY_SUBTITLE__ &lt;Ada&gt;</h1>"));
    assert!(index.contains("<p>__SANIKEY_DEFAULT_TAB__ &amp; co</p>"));
    assert!(index.contains("data-default-section=\"documents\""));
    assert!(!index.contains("__SANIKEY_DISPLAY_NAME__"));
}

#[test]
fn frontend_generator_copies_background_and_serializes_usb_info() {
    let directory = TestDirectory::new();
    let background = directory.path().join("sfondo.PNG");
    fs::write(&background, b"fixture immagine sintetica").expect("immagine sintetica");
    let mut person = person(directory.path());
    person.ui.background_image = Some(background);
    person.usb_uuid = "uuid-\"sintetico\"".to_owned();

    let result = build_frontend(&person).expect("frontend sintetico");
    let copied = result.web_dir.join("assets/background.png");
    assert_eq!(
        fs::read(copied).expect("sfondo copiato"),
        b"fixture immagine sintetica"
    );

    let script = fs::read_to_string(result.web_dir.join("usb-info.js")).expect("usb-info");
    let payload: Value = serde_json::from_str(
        script
            .strip_prefix("window.SANIKEY_USB_INFO = ")
            .and_then(|value| value.strip_suffix(";\n"))
            .expect("assegnazione JavaScript"),
    )
    .expect("payload JSON valido");
    assert_eq!(payload["usb_uuid"], "uuid-\"sintetico\"");
    assert_eq!(payload["sanikey_version"], crate::version());
    assert!(payload["exported_at"].is_null());
}

#[test]
fn vendored_frontend_assets_match_the_pinned_python_baseline() {
    let assets = [
        (
            "index.html.template",
            INDEX_TEMPLATE.as_bytes(),
            "17a8c12c29af6daabe5ab23c5fabc0d2851a2dac231aa3010f756cffb6a8559c",
        ),
        (
            "parameter-chart.html.template",
            EXTENDED_CHART_TEMPLATE.as_bytes(),
            "792901f8fcbfc3ca06c8e6ff28b1626f1fd9dc0559083bcbbbd080494e7f3d77",
        ),
        (
            "app.js",
            APP_JS.as_bytes(),
            "5b23e16e8f6dc6a3e50ca48abebe0db0835a4f02ac9de999552ce02586096324",
        ),
        (
            "parameter-chart.js",
            PARAMETER_CHART_JS.as_bytes(),
            "b355183dd51f814ec2e437a3957ebff1684861b984d18edf4072bd37d2e14d01",
        ),
        (
            "style.css",
            STYLE_CSS.as_bytes(),
            "1a13922eb8aa90b8b373bcd04c32e6d16f4f413cc2ee22470fc373cf88bf1e72",
        ),
        (
            "chart-navigation.js",
            CHART_NAVIGATION_JS.as_bytes(),
            "549a15c51273c5b8f89c713fd70ef3da914fbe9cdbd95af2540847850ad865a3",
        ),
        (
            "material-web.js",
            MATERIAL_WEB_JS.as_bytes(),
            "777503872982c115a0fe5da8e7009dec549ecf65f3b04e286f6824917f7fc5e1",
        ),
        (
            "material-web.css",
            MATERIAL_WEB_CSS.as_bytes(),
            "c9db5b5cd92d3111c7a1a7fd07a974f32fc3e5687b9e507ec6130e237272ad10",
        ),
        (
            "ui-helper.js",
            UI_HELPER_JS.as_bytes(),
            "bb180a0e113b659e7a759b25837bdd6ae9b353d8702cfbc1f7d30f03103cec70",
        ),
        (
            "chart.umd.min.js",
            CHART_JS,
            "206b6e8bb00fc7bba2c7ee80ca41db3e9e05ba7be0aa35abeba9cfd5357f5d0e",
        ),
        (
            "LICENSE.md",
            CHART_LICENSE,
            "41a84aa2caba645f966a18d9c2056b73e6d3a81d80bc0046bc0011a2634d4cce",
        ),
        (
            "sanikey-logo-horizontal-transparent.svg",
            SANKEY_LOGO,
            "bbb358a3a9e036fbd1d2d89a9440015881111cfa0f108d2b14561555463d86ed",
        ),
    ];
    for (name, bytes, expected) in assets {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected, "{name}");
    }
}

#[test]
#[ignore = "richiede Chromium headless: impostare SANIKEY_CHROMIUM_BIN"]
fn chromium_opens_offline_pages_and_exercises_chart_flows() {
    let browser = std::env::var_os("SANIKEY_CHROMIUM_BIN")
        .expect("impostare SANIKEY_CHROMIUM_BIN al percorso di Chromium");
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    person.local_build = directory.path().join("build");
    let result = build_frontend(&person).expect("frontend browser sintetico");
    let data = r#"window.SANIKEY_DATA = {
      summary: {}, timeline: [], documents: [], search: [
        {id: "doc-a", type: "document", section: "documents", title: "Referto sintetico", href: "documents/referto.pdf"}
      ], clinical: {
        observation_series: [{id: "pressure", name: "Pressione", value_type: "blood_pressure", unit: "mmHg"}],
        observation_points: [
          {id: "point-2020", series_id: "pressure", date: "2020-01-01", systolic: 120, diastolic: 80, pulse: 65, raw_value: "120/80", numeric_value: null},
          {id: "point-2025", series_id: "pressure", date: "2025-01-01", systolic: 122, diastolic: 82, pulse: null, raw_value: "122/82", numeric_value: null}
        ]
      }
    };
    window.SANIKEY_CONTENT_SEARCH = {documents: []};"#;
    fs::write(result.web_dir.join("data.js"), data).expect("dati browser sintetici");
    fs::create_dir_all(result.web_dir.join("documents")).expect("directory documenti sintetici");
    fs::write(result.web_dir.join("documents/referto.pdf"), b"fixture")
        .expect("documento sintetico collegato");

    let home_probe = r###"<script>
    (() => {
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      try {
        check(location.protocol === "file:", "la pagina deve funzionare da file://");
        const canvas = document.querySelector("#parameters canvas");
        const chart = Chart.getChart(canvas);
        check(chart, "grafico standard assente");
        check(chart.data.datasets.map(dataset => dataset.label).join(",") === "Sistolica,Diastolica,Polso", "serie pressione/polso inattese");
        check(chart.data.datasets[2].yAxisID === "y1", "il polso deve usare la scala secondaria");
        check(chart.data.datasets[2].data.length === 1, "il valore polso mancante è stato tracciato");
        const extendedLink = document.querySelector("#parameter-detail a[href^='parameter-chart.html']");
        check(extendedLink && extendedLink.href.includes("series=pressure"), "link al grafico esteso assente");
        const search = document.querySelector("#search");
        search.value = "referto sintetico";
        search.dispatchEvent(new Event("input", {bubbles: true}));
        check(document.querySelector("#documents a[href='documents/referto.pdf']"), "ricerca/link al documento non riusciti");
        const activeChart = Chart.getChart(canvas);
        document.querySelector("#parameters [data-parameter-filter='from']").value = "2099-01-01";
        document.querySelector("#parameters [data-parameter-filter='from']").dispatchEvent(new Event("input", {bubbles: true}));
        check(!Chart.getChart(canvas), "il grafico sostituito non è stato smontato");
        check(!document.querySelector("#parameters .chart-navigation"), "i controlli del grafico smontato sono rimasti");
        check(activeChart._destroyed, "Chart.js non ha marcato il grafico come smontato");
        document.body.insertAdjacentHTML("beforeend", "<p id='browser-test-result'>BROWSER_TEST_PASS</p>");
      } catch (error) {
        document.body.insertAdjacentHTML("beforeend", `<pre id='browser-test-result'>BROWSER_TEST_FAIL: ${error.message}</pre>`);
      }
    })();
    </script>"###;
    append_before_body_end(&result.index, home_probe);
    let home = run_chromium(&browser, &result.index);
    assert!(
        home.contains("BROWSER_TEST_PASS"),
        "browser home probe fallito:\n{home}"
    );

    let extended_probe = r###"<script>
    (() => {
      const check = (condition, message) => { if (!condition) throw new Error(message); };
      try {
        check(location.protocol === "file:", "il grafico esteso deve funzionare da file://");
        check(document.querySelector("a[href='index.html']"), "link di ritorno all'archivio assente");
        const chart = Chart.getChart(document.querySelector("canvas"));
        check(chart, "grafico esteso assente");
        check(chart.data.datasets.some(dataset => dataset.label === "Polso" && dataset.yAxisID === "y1"), "serie polso o asse secondario assenti");
        check(!chart.data.datasets.some(dataset => dataset.label === "Polso" && dataset.data.some(point => point.y == null)), "valore mancante presente nel grafico");
        const previous = document.querySelector(".chart-navigation [data-pan='previous']");
        previous.click();
        check(document.querySelector(".chart-navigation-empty").textContent.includes("Nessuna misurazione"), "intervallo temporale vuoto non segnalato");
        const filter = document.querySelector("[data-extended-filter='from']");
        filter.value = "2099-01-01";
        filter.dispatchEvent(new Event("input", {bubbles: true}));
        check(document.querySelector("[data-extended-charts]").textContent.includes("Nessun punto grafico"), "filtro senza dati non segnalato");
        check(!document.querySelector("[data-extended-charts] canvas"), "canvas del grafico rimosso non pulito");
        document.body.insertAdjacentHTML("beforeend", "<p id='browser-test-result'>BROWSER_TEST_PASS</p>");
      } catch (error) {
        document.body.insertAdjacentHTML("beforeend", `<pre id='browser-test-result'>BROWSER_TEST_FAIL: ${error.message}</pre>`);
      }
    })();
    </script>"###;
    append_before_body_end(&result.extended_chart, extended_probe);
    let extended = run_chromium(&browser, &result.extended_chart);
    assert!(
        extended.contains("BROWSER_TEST_PASS"),
        "browser extended chart probe fallito:\n{extended}"
    );
}

fn append_before_body_end(path: &Path, script: &str) {
    let html = fs::read_to_string(path).expect("pagina HTML browser sintetica");
    let (prefix, suffix) = html.rsplit_once("</body>").expect("chiusura body HTML");
    fs::write(path, format!("{prefix}{script}\n</body>{suffix}"))
        .expect("probe browser inserita nella pagina sintetica");
}

fn run_chromium(browser: &std::ffi::OsStr, page: &Path) -> String {
    let output = std::process::Command::new("timeout")
        .arg("20s")
        .arg(browser)
        .args([
            "--headless",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--no-first-run",
            "--no-default-browser-check",
            "--dump-dom",
            "--timeout=10000",
        ])
        .arg(format!("file://{}", page.display()))
        .output()
        .expect("avvio di Chromium");
    assert!(
        output.status.success(),
        "Chromium ha terminato con {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}
