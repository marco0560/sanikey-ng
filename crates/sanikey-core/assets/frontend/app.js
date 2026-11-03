const SECTION_LABELS = {
  documents: "Documenti",
  therapies: "Terapie",
  medications: "Farmaci",
  problems: "Problemi",
  procedures: "Procedure",
  observations: "Osservazioni",
  parameters: "Parametri",
  dicom: "Studi DICOM",
  timeline: "Timeline",
  summary: "Sintesi Clinica",
};

const SECTION_ORDER = ["documents", "therapies", "medications", "problems", "procedures", "observations", "parameters", "dicom", "timeline"];
function text(value) {
  return value === null || value === undefined ? "" : String(value);
}

function escapeHtml(value) {
  return text(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function attr(value) {
  return escapeHtml(value);
}

function html(value) {
  return value === null || value === undefined ? "" : String(value);
}

function formatDate(value) {
  const rendered = text(value);
  const match = rendered.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  return match ? `${match[3]}/${match[2]}/${match[1]}` : rendered;
}

function formatDateRange(startDate, endDate) {
  const start = formatDate(startDate);
  const end = formatDate(endDate);
  return end ? `${start} - ${end}` : start;
}

function formatTechnicalTimestamp(value) {
  const parsed = new Date(text(value));
  if (Number.isNaN(parsed.getTime())) {
    return "Non disponibile";
  }
  return new Intl.DateTimeFormat("it-IT", {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    timeZoneName: "short",
  }).format(parsed);
}

function renderUsbInfo() {
  const info = window.SANIKEY_USB_INFO || {};
  const target = document.querySelector("#usb-info-details");
  const fields = [
    ["UUID USB", text(info.usb_uuid) || "Non disponibile"],
    ["Ultimo export SaniKey", info.exported_at ? formatTechnicalTimestamp(info.exported_at) : "Anteprima locale non esportata"],
    ["Versione SaniKey", text(info.sanikey_version) || "Non disponibile"],
    ["Schema manifest", text(info.schema_version) || "Non disponibile"],
    ["Copia USB", text(info.copy_strategy) || "Non disponibile"],
  ];
  target.innerHTML = fields.map(([label, value]) =>
    `<div><dt>${escapeHtml(label)}</dt><dd>${escapeHtml(value)}</dd></div>`
  ).join("");
}

function applyUi(summary) {
  const ui = summary.ui || {};
  document.documentElement.style.setProperty("--accent", text(ui.accent_color || "#2563eb"));
  document.documentElement.style.setProperty("--background-opacity", text(ui.background_opacity || "0.1"));
  if (ui.background_image) {
    document.documentElement.style.setProperty("--background-image", `url("${attr(ui.background_image)}")`);
    document.body.classList.add("has-background-image");
  }
  document.body.dataset.density = text(ui.density || "comfortable");
  document.querySelector("main").dataset.defaultSection = text(ui.default_tab || "documents");
}

function renderSummary(summary, clinical = {}) {
  const target = document.querySelector("#summary");
  target.innerHTML = `<h2>Sintesi Clinica</h2>
    <div class="markdown">${html(summary.clinical_summary_html) || `<p>${escapeHtml(summary.clinical_summary)}</p>`}</div>
    ${renderClinicalDashboard(clinical)}
    <section class="technical-summary" aria-label="Riepilogo tecnico">
      <h3>Riepilogo tecnico</h3>
      <dl>
        <div><dt>Documenti</dt><dd>${escapeHtml(summary.document_count)}</dd></div>
        <div><dt>Problemi</dt><dd>${escapeHtml(summary.problem_count)}</dd></div>
        <div><dt>Procedure</dt><dd>${escapeHtml(summary.procedure_count)}</dd></div>
      </dl>
    </section>`;
}

function renderTimeline(timeline, documents) {
  const target = document.querySelector("#timeline");
  target.innerHTML = "<h2>Timeline</h2>" + timeline.map((item) =>
    `<article id="entity-${attr(item.id)}"><strong>${escapeHtml(formatDateRange(item.start_date, item.end_date))}</strong> ${escapeHtml(item.title)}
      ${renderTimelineLinks(item, documents)}</article>`
  ).join("");
}

function renderTimelineLinks(item, documents) {
  const links = item.links || [];
  if (!links.length) {
    return "";
  }
  const documentsById = new Map((documents || []).map((documentItem) => [documentItem.id, documentItem]));
  return `<p>${links.map((link) => {
    const documentItem = documentsById.get(link);
    if (documentItem && documentItem.href) {
      return `<a class="primary-action" href="${attr(documentItem.href)}" target="_blank" rel="noopener">Apri documento</a>`;
    }
    return `<a href="#entity-${attr(link)}" data-detail-link="${attr(link)}">Dettaglio</a>`;
  }).join(" ")}</p>`;
}

function setupTimelineDetailLinks(documents) {
  const timeline = document.querySelector("#timeline");
  timeline.addEventListener("click", (event) => {
    const link = event.target.closest("[data-detail-link]");
    if (!link) {
      return;
    }
    const detailId = text(link.dataset.detailLink);
    if (openParameterDetail(detailId)) {
      event.preventDefault();
      return;
    }
    let detail = findEntityDetail(detailId);
    if (!detail) {
      renderDocuments(documents);
      const search = document.querySelector("#search");
      search.value = "";
      detail = findEntityDetail(detailId);
    }
    const panel = detail && detail.closest("[data-section-panel]");
    if (!panel) {
      return;
    }
    event.preventDefault();
    window.SaniKeyUi.showSection(panel.dataset.sectionPanel, "left");
    requestAnimationFrame(() => detail.scrollIntoView({block: "start"}));
  });
}

function setupResultDetailLinks() {
  const results = document.querySelector("main");
  results.addEventListener("click", (event) => {
    const link = event.target.closest("[data-result-detail-link]");
    if (!link) {
      return;
    }
    const detailId = text(link.dataset.resultDetailLink);
    if (openParameterDetail(detailId)) {
      event.preventDefault();
      return;
    }
    const detail = findEntityDetail(detailId);
    const panel = detail && detail.closest("[data-section-panel]");
    if (!panel) {
      return;
    }
    event.preventDefault();
    window.SaniKeyUi.showSection(panel.dataset.sectionPanel, "left");
    requestAnimationFrame(() => detail.scrollIntoView({block: "start"}));
  });
}

function findEntityDetail(id) {
  return document.getElementById(`entity-${id}`) || document.getElementById(`summary-entity-${id}`);
}

function openParameterDetail(id) {
  const panel = document.querySelector("#parameters");
  return Boolean(panel && !panel.dispatchEvent(new CustomEvent("sanikeyselectparameter", {detail: id, cancelable: true})));
}

function renderDocuments(documents, query = "") {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const selected = sortDocumentResults(documents.filter((item) => !isDicomTechnicalDocument(item)).filter((item) =>
    terms.every((term) => documentSearchText(item).includes(term))
  ));
  const target = document.querySelector("#documents");
  const count = query ? `<p class="result-count">${selected.length} risultati</p>` : "";
  target.innerHTML = "<h2>Documenti</h2>" + count + selected.map((item) =>
    `<article id="entity-${attr(item.id)}"><h3>${escapeHtml(item.title)}</h3>
      <p>${escapeHtml(formatDate(item.date))} ${escapeHtml(item.category)} ${escapeHtml(item.kind)}</p>
      <p>${item.tags.map(escapeHtml).join(", ")}</p>
      ${item.markdown_html ? `<div class="markdown">${html(item.markdown_html)}</div>` : ""}
      ${renderDocumentActions(item)}</article>`
  ).join("");
}

function sortDocumentResults(records) {
  return [...records].sort((left, right) =>
    text(right.date).localeCompare(text(left.date))
    || text(left.title).localeCompare(text(right.title), "it")
  );
}

function renderDocumentActions(item) {
  if (item.viewer_href) {
    const label = item.native_viewer_href ? "Apri studio DICOM" : "Apri visualizzatore integrato (non diagnostico)";
    const media = item.dicomdir_href ? ` <a href="${attr(item.dicomdir_href)}">DICOMDIR per viewer professionale</a>` : "";
    return `<p class="actions"><a class="primary-action" href="${attr(item.viewer_href)}" target="_blank" rel="noopener">${label}</a>${media}</p>`;
  }
  if (item.href) {
    const original = item.source_href && item.source_href !== item.href
      ? ` <a href="${attr(item.source_href)}" target="_blank" rel="noopener">Scarica originale</a>`
      : "";
    return `<p class="actions"><a class="primary-action" href="${attr(item.href)}" target="_blank" rel="noopener">Apri documento</a>${original}</p>`;
  }
  return `<span class="muted">Origine nel contenitore</span>`;
}

function renderClinicalDashboard(clinical) {
  const sections = [
    ["problems", clinical.problems || []],
    ["therapies", clinical.therapies || []],
    ["medications", clinical.medications || []],
    ["observations", clinical.observations || []],
    ["procedures", clinical.procedures || []],
  ].filter(([, items]) => items.length);
  if (!sections.length) {
    return "";
  }
  return `<nav class="section-links" aria-label="Sezioni riepilogo">${sections.map(([section, items]) =>
    `<a href="#summary-${attr(section)}">${escapeHtml(SECTION_LABELS[section])} ${items.length}</a>`
  ).join("")}</nav>` + sections.map(([section, items]) =>
    `<section id="summary-${attr(section)}" class="summary-section"><h3>${escapeHtml(SECTION_LABELS[section])}</h3>
      ${items.map((item) => renderEntityCard(item, section, "summary-entity")).join("")}</section>`
  ).join("");
}

function renderDicomStudies(studies) {
  const target = document.querySelector("#dicom");
  const selected = sortDicomStudies(studies);
  target.innerHTML = `<h2>Studi DICOM</h2>` + (
    selected.length
      ? selected.map(renderDicomStudyCard).join("")
      : '<p class="muted">Nessuno studio DICOM disponibile.</p>'
  );
}

function observationSectionForSeries(series) {
  return "parameters";
}

function renderObservationSections(clinical) {
  const series = clinical.observation_series || [];
  const points = clinical.observation_points || [];
  const bySeries = new Map(series.map((item) => [item.id, item]));
  const grouped = {parameters: []};
  points.forEach((point) => {
    const itemSeries = bySeries.get(point.series_id) || {id: point.series_id, name: point.series_id};
    grouped[observationSectionForSeries(itemSeries)].push({series: itemSeries, point});
  });
  Object.entries(grouped).forEach(([section, items]) => renderObservationSection(section, items));
  configureObservationNavigation(grouped);
}

function renderObservationSection(section, items) {
  const target = document.querySelector(`#${section}`);
  const sorted = [...items].sort((left, right) => text(right.point.date).localeCompare(text(left.point.date)));
  if (section === "parameters") {
    renderParameterSection(target, sorted);
    return;
  }
  target.innerHTML = `<h2>${escapeHtml(SECTION_LABELS[section])}</h2>` + (
    sorted.length
      ? `<table class="observation-table"><thead><tr><th>Data</th><th>Serie</th><th>Valore</th><th>Fonte</th></tr></thead><tbody>${sorted.map(({series, point}) =>
          `<tr id="entity-${attr(point.id)}"><td>${escapeHtml(formatDate(point.date))}</td><td>${escapeHtml(series.name || series.id)}</td><td>${escapeHtml(observationDisplayValue(point))}</td><td>${renderObservationSource(point)}</td></tr>`
        ).join("")}</tbody></table>`
      : '<p class="muted">Nessuna misurazione disponibile.</p>'
  );
}

function renderParameterSection(target, items) {
  const series = [...new Map(items.map(({series}) => [series.id, series])).values()]
    .sort((left, right) => text(left.name || left.id).localeCompare(text(right.name || right.id), "it"));
  let selectedSeriesId = series[0] && series[0].id;
  let query = "";
  const defaultFilters = {from: "", to: "", unit: "", category: "", qualified: "all", order: "desc"};
  const filters = {...defaultFilters};
  const units = [...new Set(items.map(({point, series}) => parameterUnit(point, series)).filter(Boolean))].sort();
  const categories = [...new Set(items.map(({point}) => point.document_category).filter(Boolean))].sort();
  const detailPanel = document.querySelector("#parameter-detail");
  target.innerHTML = `<h2>Parametri</h2>
    <label class="parameter-search">Cerca parametro o sinonimo
      <input type="search" data-parameter-search autocomplete="off">
    </label>
    <fieldset class="parameter-filters"><legend>Filtri</legend>
      <label>Da <input type="date" data-parameter-filter="from"></label><label>A <input type="date" data-parameter-filter="to"></label>
      <label>Unita' <select data-parameter-filter="unit"><option value="">Tutte</option>${units.map((value) => `<option value="${attr(value)}">${escapeHtml(value)}</option>`).join("")}</select></label>
      <label>Categoria <select data-parameter-filter="category"><option value="">Tutte</option>${categories.map((value) => `<option value="${attr(value)}">${escapeHtml(value)}</option>`).join("")}</select></label>
      <label>Qualificati <select data-parameter-filter="qualified"><option value="all">Tutti</option><option value="hide">Nascondi</option><option value="only">Solo qualificati</option></select></label>
      <label>Ordine <select data-parameter-filter="order"><option value="desc">Piu' recenti</option><option value="asc">Meno recenti</option></select></label>
    </fieldset>
    <div class="parameter-layout"><nav class="parameter-list" aria-label="Serie di parametri"></nav>
      <div class="parameter-content"></div></div>`;
  const search = target.querySelector("[data-parameter-search]");
  const list = target.querySelector(".parameter-list");
  const content = target.querySelector(".parameter-content");
  const layout = target.querySelector(".parameter-layout");
  const detailHeading = document.createElement("h2");
  detailHeading.textContent = "Dettaglio parametro";
  const arrange = () => {
    if (document.body.dataset.layout === "dual") {
      detailPanel.replaceChildren(detailHeading, content);
      return;
    }
    layout.appendChild(content);
    detailPanel.replaceChildren();
  };
  const render = () => {
    const normalized = normalizeSearchText(query);
    const visible = series.filter((item) => parameterSearchText(item).includes(normalized));
    if (!visible.some((item) => item.id === selectedSeriesId)) {
      selectedSeriesId = visible[0] && visible[0].id;
    }
    list.innerHTML = visible.length ? visible.map((item) => {
      const count = items.filter(({series}) => series.id === item.id).length;
      return `<button id="entity-${attr(item.id)}" type="button" data-parameter-series="${attr(item.id)}" aria-pressed="${item.id === selectedSeriesId}">${escapeHtml(item.name || item.id)} <span>${count}</span></button>`;
    }).join("") : '<p class="muted">Nessun parametro corrispondente.</p>';
    renderSelectedParameter(content, items, selectedSeriesId, filters);
    list.querySelectorAll("[data-parameter-series]").forEach((button) => button.addEventListener("click", () => {
      selectedSeriesId = button.dataset.parameterSeries;
      render();
    }));
  };
  search.addEventListener("input", () => {
    query = search.value;
    render();
  });
  target.querySelectorAll("[data-parameter-filter]").forEach((control) => control.addEventListener("input", () => {
    filters[control.dataset.parameterFilter] = control.value;
    render();
  }));
  target.addEventListener("sanikeyselectparameter", (event) => {
    const item = items.find(({series, point}) => series.id === event.detail || point.id === event.detail);
    if (!item) { return; }
    event.preventDefault();
    selectedSeriesId = item.series.id;
    query = "";
    search.value = "";
    Object.assign(filters, defaultFilters);
    target.querySelectorAll("[data-parameter-filter]").forEach((control) => {
      control.value = filters[control.dataset.parameterFilter];
    });
    render();
    window.SaniKeyUi.showSection("parameters", "left");
    const detail = findEntityDetail(event.detail);
    if (detail?.matches("[data-parameter-point]")) { detail.click(); }
    requestAnimationFrame(() => detail?.scrollIntoView({block: "start"}));
  });
  window.addEventListener("sanikeylayoutchange", arrange);
  render();
  arrange();
}

function parameterSearchText(series) {
  return normalizeSearchText([series.name, series.id, ...(series.synonyms || [])].join(" "));
}

function parameterUnit(point, series) {
  return point.normalized_unit || point.raw_unit || series.unit || "";
}

function hasNumericValue(value) {
  return value !== null && value !== undefined && text(value).trim() !== "" && Number.isFinite(Number(value));
}

function destroyParameterCharts(target) {
  if (typeof Chart !== "undefined") {
    target.querySelectorAll("canvas").forEach((canvas) => Chart.getChart(canvas)?.destroy());
  }
}

function renderSelectedParameter(target, items, seriesId, filters) {
  destroyParameterCharts(target);
  const selected = items.filter(({series, point}) => series.id === seriesId
    && (!filters.from || point.date >= filters.from)
    && (!filters.to || point.date <= filters.to)
    && (!filters.unit || parameterUnit(point, series) === filters.unit)
    && (!filters.category || point.document_category === filters.category)
    && (filters.qualified === "all" || (filters.qualified === "only") === Boolean(point.qualifier)))
    .sort((left, right) => filters.order === "asc"
      ? text(left.point.date).localeCompare(text(right.point.date))
      : text(right.point.date).localeCompare(text(left.point.date)));
  if (!selected.length) {
    target.innerHTML = seriesId
      ? '<p class="muted">Nessuna misurazione corrisponde ai filtri selezionati.</p>'
      : '<p class="muted">Selezionare un parametro.</p>';
    return;
  }
  const series = selected[0].series;
  target.innerHTML = `<h3>${escapeHtml(series.name || series.id)}</h3>
    ${series.synonyms && series.synonyms.length ? `<p class="muted">Sinonimi: ${escapeHtml(series.synonyms.join(", "))}</p>` : ""}
    <div class="parameter-chart-host"></div><div class="parameter-detail" aria-live="polite"></div>
    <div class="parameter-table-scroll"><table class="observation-table"><thead><tr><th>Data</th><th>Valore originale</th><th>Unita'</th><th>Documento</th><th>Etichetta trovata</th><th>Provenienza</th></tr></thead><tbody>${selected.map(({point}) =>
      `<tr id="entity-${attr(point.id)}" tabindex="0" data-parameter-point="${attr(point.id)}"><td>${escapeHtml(formatDate(point.date))}</td><td>${escapeHtml((point.qualifier || "") + (point.raw_value || point.value || ""))}</td><td>${escapeHtml(point.raw_unit || point.normalized_unit || series.unit || "")}</td><td>${point.document_href ? `<a href="${attr(point.document_href)}" target="_blank" rel="noopener">Apri documento</a>` : escapeHtml(point.document_title || "")}</td><td>${escapeHtml(point.matched_label || "")}</td><td>${escapeHtml(point.source_reference || "")}</td></tr>`
    ).join("")}</tbody></table></div>`;
  const detail = target.querySelector(".parameter-detail");
  const showPoint = (point) => {
    detail.innerHTML = renderParameterDetail(point, series);
    window.SaniKeyCharts.reveal(target.querySelector("canvas"), Date.parse(point.date + "T00:00:00"));
  };
  target.querySelectorAll("[data-parameter-point]").forEach((row) => {
    const point = selected.find(({point: item}) => item.id === row.dataset.parameterPoint).point;
    row.addEventListener("click", () => showPoint(point));
    row.addEventListener("keydown", (event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); showPoint(point); } });
  });
  showPoint(selected[0].point);
  renderParameterChart(target.querySelector(".parameter-chart-host"), selected, showPoint);
}

function renderParameterDetail(point, series) {
  const source = [pointOrigin(point), point.matched_label && "Etichetta: " + point.matched_label].filter(Boolean).join(" · ");
  const action = point.document_href ? `<p class="actions"><a class="primary-action" href="${attr(point.document_href)}" target="_blank" rel="noopener">Apri documento</a></p>` : "";
  const extended = `<p class="actions"><a class="primary-action" href="parameter-chart.html?series=${encodeURIComponent(series.id)}" target="_blank" rel="noopener">Apri grafico esteso</a></p>`;
  return `<h4>Dettaglio misurazione</h4><p>${escapeHtml(observationDisplayValue(point, series.unit))}</p><p class="muted">${escapeHtml(source)}</p>${extended}${action}`;
}

function observationDisplayValue(point, seriesUnit = "") {
  const raw = point.raw_value || point.value;
  const qualifier = point.qualifier || "";
  const unit = point.raw_unit || point.normalized_unit || seriesUnit;
  const includesUnit = unit && text(raw).trim().toLowerCase().endsWith(unit.toLowerCase());
  return qualifier + raw + (unit && !includesUnit ? " " + unit : "");
}

function pointOrigin(point) {
  if (point.document_href) {
    const fileName = decodeURIComponent(text(point.document_href).split("/").pop());
    return "Documento: " + fileName;
  }
  if (point.source_reference) {
    return "Origine: " + point.source_reference;
  }
  if (point.source_kind === "curated-observation") {
    return "Origine: osservazione curata";
  }
  return "Origine: non disponibile";
}

function renderObservationSource(point) {
  const details = [
    point.matched_label ? "Etichetta: " + point.matched_label : "",
    point.rule_id ? "Regola: " + point.rule_id : "",
    point.source_reference || "",
  ].filter(Boolean).join(" · ");
  const detail = escapeHtml(details);
  if (!point.document_href) {
    return detail;
  }
  const link = '<a href="' + attr(point.document_href) + '" target="_blank" rel="noopener">Apri originale</a>';
  return link + (detail ? "<br><small>" + detail + "</small>" : "");
}

function renderParameterChart(target, items, onPointSelected) {
  if (typeof Chart === "undefined") {
    return;
  }
  const bloodPressure = items[0] && items[0].series.value_type === "blood_pressure";
  const numeric = items.filter(({point}) => hasNumericValue(point.numeric_value));
  if (!bloodPressure && !numeric.length) {
    return;
  }
  const canvas = document.createElement("canvas");
  canvas.className = "parameter-chart";
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", "Grafico temporale dei parametri disponibili");
  const plot = document.createElement("div");
  plot.className = "parameter-chart-plot";
  plot.appendChild(canvas);
  target.appendChild(plot);
  const datasets = bloodPressure ? bloodPressureDatasets(items) : numericDatasets(numeric);
  if (!datasets.length) {
    return;
  }
  window.SaniKeyCharts.create(canvas, {
    type: "line",
    data: {datasets},
    options: {
      responsive: true,
      maintainAspectRatio: false,
      scales: {
        x: {
          type: "linear",
          ticks: {callback: (value) => new Date(Number(value)).toLocaleDateString("it-IT")},
        },
        ...(bloodPressure ? {
          y: {type: "linear", position: "left", title: {display: true, text: items[0].series.unit || "mmHg"}},
          ...(datasets.some((dataset) => dataset.yAxisID === "y1") ? {
            y1: {type: "linear", position: "right", title: {display: true, text: "bpm"}, grid: {drawOnChartArea: false}},
          } : {}),
        } : {}),
      },
      plugins: {
        tooltip: {
          callbacks: {
            title: (contexts) => contexts.length ? formatDate(contexts[0].raw.point.date) : "",
            label: (context) => chartPointLabel(context.raw),
            afterLabel: (context) => pointOrigin(context.raw.point),
          },
        },
      },
      onClick: (_event, elements, chart) => {
        if (!elements.length) { return; }
        const element = elements[0];
        const point = chart.data.datasets[element.datasetIndex].data[element.index].point;
        if (!point) { return; }
        if (point.document_href) {
          window.open(point.document_href, "_blank", "noopener");
          return;
        }
        onPointSelected(point);
      },
    },
  }, bloodPressure ? 31 : 92);
}

function numericDatasets(numeric) {
  const grouped = new Map();
  numeric.forEach(({series, point}) => {
    const unit = point.normalized_unit || point.raw_unit || series.unit || "";
    const key = series.id + "|" + unit;
    const name = series.name || series.id;
    const label = unit && unit !== series.unit ? name + " (" + unit + ")" : name;
    const entries = grouped.get(key) || {label, series, points: []};
    entries.points.push(point);
    grouped.set(key, entries);
  });
  const datasets = [];
  const colors = ["#1d4ed8", "#dc2626", "#047857", "#7c3aed", "#b45309"];
  grouped.forEach((entry, index) => {
    const color = colors[index % colors.length];
    const regular = entry.points.filter((point) => !point.qualifier);
    const qualified = entry.points.filter((point) => point.qualifier);
    if (regular.length) {
      datasets.push({
        label: entry.label,
        data: regular.map((point) => ({x: Date.parse(point.date + "T00:00:00"), y: Number(point.numeric_value), point, series: entry.series})),
        borderColor: color,
        backgroundColor: color,
        pointBackgroundColor: color,
        pointBorderColor: "#ffffff",
        pointBorderWidth: 1.5,
        pointRadius: 4,
        pointHoverRadius: 7,
        pointHitRadius: 8,
        borderWidth: 2,
        tension: 0,
      });
    }
    if (qualified.length) {
      datasets.push({
        label: entry.label + " (qualificati)",
        data: qualified.map((point) => ({x: Date.parse(point.date + "T00:00:00"), y: Number(point.numeric_value), point, series: entry.series})),
        borderColor: color,
        backgroundColor: color,
        pointBackgroundColor: color,
        pointBorderColor: "#ffffff",
        pointBorderWidth: 1.5,
        showLine: false,
        pointStyle: "triangle",
        pointRadius: 6,
      });
    }
  });
  return datasets;
}

function bloodPressureDatasets(items) {
  const components = [
    {field: "systolic", label: "Sistolica", color: "#1d4ed8"},
    {field: "diastolic", label: "Diastolica", color: "#dc2626"},
    {field: "pulse", label: "Polso", color: "#15803d"},
  ];
  return components.map((component) => {
    const data = items.filter(({point}) => hasNumericValue(point[component.field]))
      .map(({series, point}) => ({x: Date.parse(point.date + "T00:00:00"), y: Number(point[component.field]), point, series, component}));
    return data.length ? {
      label: component.label,
      yAxisID: component.field === "pulse" ? "y1" : "y",
      data,
      borderColor: component.color,
      backgroundColor: component.color,
      pointBackgroundColor: component.color,
      pointBorderColor: "#ffffff",
      pointBorderWidth: 1.5,
      pointRadius: 4,
      pointHoverRadius: 7,
      pointHitRadius: 8,
      borderWidth: 2,
      tension: 0,
    } : null;
  }).filter(Boolean);
}

function chartPointLabel(raw) {
  if (raw.component) {
    const unit = raw.component.field === "pulse" ? "bpm" : (raw.series.unit || "mmHg");
    return raw.component.label + ": " + raw.point[raw.component.field] + " " + unit;
  }
  return observationDisplayValue(raw.point, raw.series.unit);
}

function configureObservationNavigation(grouped) {
  Object.entries(grouped).forEach(([section, items]) => {
    document.querySelectorAll(`[data-observation-control="${section}"]`).forEach((control) => {
      control.hidden = !items.length;
    });
    document.querySelector(`#${section}`).hidden = !items.length;
    if (section === "parameters") {
      document.querySelector("#parameter-detail").hidden = !items.length;
    }
  });
}

function renderTherapies(therapies) {
  const target = document.querySelector("#therapies");
  const selected = therapies || [];
  target.innerHTML = `<h2>Terapia</h2>` + (
    selected.length
      ? selected.map((item) => renderEntityCard(item, "therapies")).join("")
      : '<p class="muted">Nessuna terapia disponibile.</p>'
  );
}

function configureDicomNavigation(studies) {
  const hasDicom = (studies || []).length > 0;
  document.querySelectorAll("[data-dicom-control]").forEach((control) => {
    control.hidden = !hasDicom;
  });
  document.querySelector("#dicom").hidden = !hasDicom;
}

function configureTherapyNavigation(therapies) {
  const hasTherapy = (therapies || []).length > 0;
  document.querySelectorAll("[data-therapy-control]").forEach((control) => {
    control.hidden = !hasTherapy;
  });
  document.querySelector("#therapies").hidden = !hasTherapy;
}

function sortDicomStudies(studies) {
  return [...(studies || [])].sort((left, right) =>
    Number(!left.viewer_href) - Number(!right.viewer_href)
  );
}

function renderDicomStudyCard(item) {
  const anomaly = item.viewer_href || item.dicomdir_href
    ? ""
    : '<p class="warning">Anomalia: nessun viewer, anteprima o DICOMDIR disponibile per lo studio.</p>';
  return `<article id="entity-${attr(item.id)}"><h4>${escapeHtml(item.title)}</h4>
    ${item.date ? `<p>${escapeHtml(formatDate(item.date))}</p>` : ""}
    <details><summary>Dettagli tecnici</summary>${renderFields(item.fields || [])}</details>
    ${renderEntityActions(item, "dicom")}${anomaly}</article>`;
}

function recordKind(item) {
  if (item.kind) {
    return text(item.kind);
  }
  const typeField = (item.fields || []).find((field) => field.label === "Tipo");
  return typeField ? text(typeField.value) : "";
}

function isDicomTechnicalDocument(item) {
  if (item.type !== "document") {
    return false;
  }
  return recordKind(item).startsWith("dicom_");
}

function renderEntityCard(item, section, idPrefix = "entity") {
  return `<article id="${idPrefix}-${attr(item.id)}"><h4>${escapeHtml(item.title)}</h4>
    ${item.date || item.start_date ? `<p>${escapeHtml(formatDate(item.date || item.start_date))}</p>` : ""}
    ${renderFields(item.fields || [])}
    ${renderEntityActions(item, section)}</article>`;
}

function renderEntityActions(item, section) {
  if (section === "dicom") {
    const viewer = item.viewer_href
      ? `<a class="primary-action" href="${attr(item.viewer_href)}" target="_blank" rel="noopener">${item.native_viewer_href ? "Apri studio DICOM" : "Apri visualizzatore integrato (non diagnostico)"}</a>`
      : "";
    const media = item.dicomdir_href
      ? ` <a href="${attr(item.dicomdir_href)}" target="_blank" rel="noopener">DICOMDIR per viewer professionale</a>`
      : "";
    return viewer || media ? `<p class="actions">${viewer}${media}</p>` : "";
  }
  if (section !== "dicom" && item.href) {
    return `<p class="actions"><a class="primary-action" href="${attr(item.href)}" target="_blank" rel="noopener">Apri originale</a></p>`;
  }
  if (item.href) {
    return `<p class="actions"><a class="primary-action" href="${attr(item.href)}">Supporto originale per verifica tecnica</a></p>`;
  }
  if (section === "therapies" && item.leaflet_href) {
    const downloaded = item.leaflet_downloaded_at ? ` scaricato il ${escapeHtml(formatDate(item.leaflet_downloaded_at))}` : "";
    return `<p class="actions"><a href="${attr(item.leaflet_href)}" target="_blank" rel="noopener">Foglio illustrativo${downloaded}</a>${item.rcp_href ? ` <a href="${attr(item.rcp_href)}" target="_blank" rel="noopener">RCP</a>` : ""} <a href="${attr(item.aifa_fi_url)}" target="_blank" rel="noopener">Verifica su AIFA</a></p>`;
  }
  if (section === "therapies" && item.non_aifa) {
    return '<p class="muted">Nessun foglio illustrativo AIFA applicabile.</p>';
  }
  return "";
}

function renderFields(fields) {
  const selected = (fields || []).filter((field) => field.value !== null && field.value !== undefined && text(field.value) !== "");
  if (!selected.length) {
    return "";
  }
  return `<dl>${selected.map((field) =>
    `<div><dt>${escapeHtml(field.label)}</dt><dd>${escapeHtml(field.value)}</dd></div>`
  ).join("")}</dl>`;
}

function renderSearchResults(target, records, heading, emptyMessage) {
  const grouped = groupBySection(records);
  const sections = SECTION_ORDER
    .filter((section) => grouped[section] && grouped[section].length)
    .map((section) => [section, grouped[section]]);
  if (!sections.length) {
    target.innerHTML = `<h2>${escapeHtml(heading)}</h2><p class="muted">${escapeHtml(emptyMessage)}</p>`;
    return;
  }
  const total = records.length;
  target.innerHTML = `<h2>${escapeHtml(heading)}</h2><p class="result-count">${total} risultati</p>
    <nav class="section-links" aria-label="Sezioni risultati">${sections.map(([section, items]) =>
      `<a href="#${attr(target.id)}-results-${attr(section)}">${escapeHtml(SECTION_LABELS[section])} ${items.length}</a>`
    ).join("")}</nav>` + sections.map(([section, items]) =>
    `<section id="${attr(target.id)}-results-${attr(section)}"><h3>${escapeHtml(SECTION_LABELS[section])}</h3>
        ${items.map((item) => renderResultCard(item, section)).join("")}</section>`
    ).join("");
}

function renderResultCard(item, section) {
  return `<article><h4>${escapeHtml(item.title)} <span class="badge">${escapeHtml(SECTION_LABELS[section] || item.type)}</span></h4>
    ${item.subtitle ? `<p>${escapeHtml(item.subtitle)}</p>` : ""}
    ${renderFields(item.fields || [])}
    ${renderResultAction(item)}</article>`;
}

function renderResultAction(item) {
  if (item.viewer_href) {
    return `<a class="primary-action" href="${attr(item.viewer_href)}" target="_blank" rel="noopener">${item.native_viewer_href ? "Apri studio DICOM" : "Apri visualizzatore integrato (non diagnostico)"}</a>`;
  }
  if (item.type === "dicom_study" && item.dicomdir_href) {
    return `<a class="primary-action" href="${attr(item.dicomdir_href)}" target="_blank" rel="noopener">DICOMDIR per viewer professionale</a>`;
  }
  if (item.type === "document" && item.href) {
    return `<a class="primary-action" href="${attr(item.href)}" target="_blank" rel="noopener">Apri documento</a>`;
  }
  return `<a href="#entity-${attr(item.id)}" data-result-detail-link="${attr(item.id)}">Vai alla scheda</a>`;
}

function groupBySection(records) {
  return records.reduce((grouped, item) => {
    const section = item.section || "documents";
    grouped[section] = grouped[section] || [];
    grouped[section].push(item);
    return grouped;
  }, {});
}

function quickSearchText(item) {
  return normalizeSearchText([
    item.title,
    item.subtitle,
    item.text,
    item.date,
    ...(item.tags || []),
    ...((item.fields || []).map((field) => field.value)),
  ].join(" "));
}

function renderQuickSearch(records, query) {
  const terms = normalizeSearchText(query).split(/\s+/).filter(Boolean);
  const selected = records.filter((item) =>
    terms.every((term) => quickSearchText(item).includes(term))
  );
  const documents = sortDocumentResults(selected.filter((item) => item.type === "document"));
  const otherRecords = selected.filter((item) => item.type !== "document");
  renderSearchResults(
    document.querySelector("#documents"),
    [...documents, ...otherRecords],
    "Risultati",
    "Nessun risultato nella ricerca rapida.",
  );
}

function documentSearchText(item) {
  return [
    item.title,
    item.category,
    item.kind,
    item.path,
    item.date,
    ...(item.tags || []),
  ].map(text).join(" ").toLowerCase();
}

function normalizeSearchText(value) {
  return text(value)
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
}

function tokenizeAdvancedQuery(query) {
  const tokens = [];
  let index = 0;
  while (index < query.length) {
    const char = query[index];
    if (/\s/.test(char)) {
      index += 1;
      continue;
    }
    if (char === "(" || char === ")") {
      tokens.push({type: char, value: char});
      index += 1;
      continue;
    }
    if (char === '"') {
      let end = index + 1;
      let value = "";
      while (end < query.length && query[end] !== '"') {
        value += query[end];
        end += 1;
      }
      if (end >= query.length) {
        throw new Error("Virgolette non chiuse nella ricerca avanzata.");
      }
      tokens.push({type: "TERM", value});
      index = end + 1;
      continue;
    }
    let end = index;
    let value = "";
    while (end < query.length && !/\s|\(|\)/.test(query[end])) {
      value += query[end];
      end += 1;
    }
    const upper = value.toUpperCase();
    tokens.push(["AND", "OR", "NOT"].includes(upper)
      ? {type: upper, value: upper}
      : {type: "TERM", value});
    index = end;
  }
  return insertImplicitAnd(tokens);
}

function insertImplicitAnd(tokens) {
  const result = [];
  tokens.forEach((token, index) => {
    const previous = tokens[index - 1];
    if (previous && canEndExpression(previous) && canStartExpression(token)) {
      result.push({type: "AND", value: "AND"});
    }
    result.push(token);
  });
  return result;
}

function canEndExpression(token) {
  return token.type === "TERM" || token.type === ")";
}

function canStartExpression(token) {
  return token.type === "TERM" || token.type === "(" || token.type === "NOT";
}

function parseAdvancedQuery(query) {
  const tokens = tokenizeAdvancedQuery(query);
  let position = 0;

  function peek() {
    return tokens[position];
  }

  function consume(type) {
    if (peek() && peek().type === type) {
      position += 1;
      return true;
    }
    return false;
  }

  function parseExpression() {
    return parseOr();
  }

  function parseOr() {
    let node = parseAnd();
    while (consume("OR")) {
      node = {type: "OR", left: node, right: parseAnd()};
    }
    return node;
  }

  function parseAnd() {
    let node = parseNot();
    while (consume("AND")) {
      node = {type: "AND", left: node, right: parseNot()};
    }
    return node;
  }

  function parseNot() {
    if (consume("NOT")) {
      return {type: "NOT", child: parseNot()};
    }
    return parsePrimary();
  }

  function parsePrimary() {
    const token = peek();
    if (!token) {
      throw new Error("Query avanzata incompleta.");
    }
    if (consume("(")) {
      const node = parseExpression();
      if (!consume(")")) {
        throw new Error("Parentesi non chiusa nella ricerca avanzata.");
      }
      return node;
    }
    if (token.type === "TERM") {
      position += 1;
      return {type: "TERM", value: normalizeSearchText(token.value)};
    }
    throw new Error(`Token inatteso nella ricerca avanzata: ${escapeHtml(token.value)}`);
  }

  if (tokens.length === 0) {
    return null;
  }
  const expression = parseExpression();
  if (position !== tokens.length) {
    throw new Error(`Sintassi non valida vicino a ${escapeHtml(tokens[position].value)}.`);
  }
  return expression;
}

function advancedSearchTerms(dictionary) {
  const mappings = new Map();
  const addGroup = (items) => {
    Object.entries(items || {}).forEach(([key, values]) => {
      const group = [key, ...(values || [])].map(normalizeSearchText).filter(Boolean);
      group.forEach((value) => mappings.set(value, group));
    });
  };
  addGroup(defaultMonthDictionary());
  addGroup(dictionary.months || {});
  addGroup(dictionary.terms || {});
  return mappings;
}

function defaultMonthDictionary() {
  return {
    gennaio: ["01", "1"],
    febbraio: ["02", "2"],
    marzo: ["03", "3"],
    aprile: ["04", "4"],
    maggio: ["05", "5"],
    giugno: ["06", "6"],
    luglio: ["07", "7"],
    agosto: ["08", "8"],
    settembre: ["09", "9"],
    ottobre: ["10"],
    novembre: ["11"],
    dicembre: ["12"],
  };
}

function evaluateAdvancedExpression(node, haystack, expansions) {
  if (node === null) {
    return true;
  }
  if (node.type === "TERM") {
    const expanded = expansions.get(node.value) || [node.value];
    return expanded.some((term) => haystack.includes(term));
  }
  if (node.type === "AND") {
    return evaluateAdvancedExpression(node.left, haystack, expansions)
      && evaluateAdvancedExpression(node.right, haystack, expansions);
  }
  if (node.type === "OR") {
    return evaluateAdvancedExpression(node.left, haystack, expansions)
      || evaluateAdvancedExpression(node.right, haystack, expansions);
  }
  if (node.type === "NOT") {
    return !evaluateAdvancedExpression(node.child, haystack, expansions);
  }
  return false;
}

function collectPositiveTerms(node) {
  if (!node) {
    return [];
  }
  if (node.type === "TERM") {
    return [node.value];
  }
  if (node.type === "NOT") {
    return [];
  }
  return [...collectPositiveTerms(node.left), ...collectPositiveTerms(node.right)];
}

function advancedHaystack(item) {
  return normalizeSearchText([
    item.title,
    item.date,
    item.category,
    item.kind,
    item.path,
    item.text,
    ...(item.tags || []),
  ].join(" "));
}

function advancedSnippet(item, terms, expansions) {
  const source = text(item.text).replace(/\s+/g, " ").trim();
  const normalized = normalizeSearchText(source);
  const expandedTerms = terms.flatMap((term) => expansions.get(term) || [term]);
  const matchIndex = expandedTerms
    .map((term) => normalized.indexOf(term))
    .filter((index) => index >= 0)
    .sort((a, b) => a - b)[0];
  const start = Math.max(0, (matchIndex || 0) - 90);
  const excerpt = source.slice(start, start + 240);
  return `${start > 0 ? "... " : ""}${excerpt}${start + 240 < source.length ? " ..." : ""}`;
}

function loadAdvancedSearchData() {
  if (window.SANIKEY_CONTENT_SEARCH) {
    return Promise.resolve(window.SANIKEY_CONTENT_SEARCH);
  }
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "content-search.js";
    script.onload = () => resolve(window.SANIKEY_CONTENT_SEARCH);
    script.onerror = () => reject(new Error("Indice di ricerca avanzata non disponibile."));
    document.body.appendChild(script);
  });
}

function setSearchMode(mode) {
  const panel = document.querySelector(".search-panel");
  panel.dataset.searchMode = mode;
  document.body.dataset.searchMode = mode;
  document.querySelector("#basic-toggle").classList.toggle("is-active", mode === "basic");
  document.querySelector("#advanced-toggle").classList.toggle("is-active", mode === "advanced");
  if (mode === "basic") {
    document.querySelector("#search").focus();
    window.SaniKeyUi.showSection("documents", "left");
  } else {
    document.querySelector("#advanced-search").focus();
    window.SaniKeyUi.showSection("advanced", "left");
  }
}

function openHelpDialog(id) {
  const dialog = document.querySelector(id);
  if (dialog.showModal) {
    dialog.showModal();
  } else {
    dialog.setAttribute("open", "");
  }
}

function closeHelpDialog(id) {
  const dialog = document.querySelector(`#${id}`);
  if (dialog.close) {
    dialog.close();
  } else {
    dialog.removeAttribute("open");
  }
}

function advancedDocumentRecord(item, terms, expansions) {
  return {
    id: item.id,
    type: "document",
    section: item.viewer_href ? "dicom" : "documents",
    title: item.title,
    subtitle: [formatDate(item.date), item.category, item.kind].filter(Boolean).join(" "),
    date: item.date,
    text: item.text,
    href: item.href,
    viewer_href: item.viewer_href,
    support_href: item.support_href,
    primary_href: item.primary_href,
    primary_action: item.primary_action,
    fields: [
      {label: "Categoria", value: item.category},
      {label: "Tipo", value: item.kind},
      {label: "Estratto", value: advancedSnippet(item, terms, expansions)},
    ],
  };
}

function renderAdvancedResults(payload, query, clinicalRecords = []) {
  const target = document.querySelector("#advanced-results");
  if (!query.trim()) {
    target.innerHTML = '<p class="muted">Inserisci una query per cercare nel testo estratto e OCR.</p>';
    return;
  }
  let expression;
  try {
    expression = parseAdvancedQuery(query);
  } catch (error) {
    target.innerHTML = `<p class="error">${escapeHtml(error.message)}</p>`;
    return;
  }
  const expansions = advancedSearchTerms(payload.dictionary || {});
  const terms = collectPositiveTerms(expression);
  const documentMatches = (payload.documents || [])
    .filter((item) => evaluateAdvancedExpression(expression, advancedHaystack(item), expansions))
    .map((item) => advancedDocumentRecord(item, terms, expansions));
  const clinicalMatches = clinicalRecords.filter((item) =>
    evaluateAdvancedExpression(expression, quickSearchText(item), expansions)
  );
  renderSearchResults(
    target,
    [...documentMatches, ...clinicalMatches],
    "Risultati ricerca avanzata",
    "Nessun risultato nella ricerca avanzata.",
  );
}

function main() {
  const data = window.SANIKEY_DATA;
  if (!data) {
    throw new Error("Dati archivio non disponibili. Rigenerare l'export USB.");
  }
  const summary = data.summary || {};
  const timeline = data.timeline || [];
  const documents = data.documents || [];
  const dicomStudies = (data.clinical || {}).dicom_studies || [];
  const therapies = (data.clinical || {}).therapies || [];
  const searchRecords = (data.search || []).filter((item) => !isDicomTechnicalDocument(item));
  const clinicalRecords = searchRecords.filter((item) => item.type !== "document");
  const quickRecords = searchRecords;
  applyUi(summary);
  renderUsbInfo();
  renderSummary(summary, data.clinical || {});
  renderTimeline(timeline, documents);
  renderTherapies(therapies);
  configureTherapyNavigation(therapies);
  renderDicomStudies(dicomStudies);
  configureDicomNavigation(dicomStudies);
  renderObservationSections(data.clinical || {});
  renderDocuments(documents);
  setupTimelineDetailLinks(documents);
  setupResultDetailLinks();
  const advancedInput = document.querySelector("#advanced-search");
  const advancedResults = document.querySelector("#advanced-results");
  advancedResults.innerHTML = '<p class="muted">La ricerca avanzata carica il testo estratto al primo uso.</p>';
  window.SaniKeyUi.setupSections({
    defaultSection: document.querySelector("main").dataset.defaultSection || "documents",
    defaultRight: "timeline",
  });
  setSearchMode("basic");
  document.querySelector("#basic-toggle").addEventListener("click", () => setSearchMode("basic"));
  document.querySelector("#advanced-toggle").addEventListener("click", () => setSearchMode("advanced"));
  document.querySelector("#basic-help-button").addEventListener("click", () => openHelpDialog("#basic-help-dialog"));
  document.querySelector("#advanced-help-button").addEventListener("click", () => openHelpDialog("#advanced-help-dialog"));
  document.querySelector("#usb-info-button").addEventListener("click", () => openHelpDialog("#usb-info-dialog"));
  document.querySelectorAll("[data-close-dialog]").forEach((button) => {
    button.addEventListener("click", () => closeHelpDialog(button.dataset.closeDialog));
  });
  document.querySelector("#search").addEventListener("input", (event) => {
    setSearchMode("basic");
    if (event.target.value.trim()) {
      renderQuickSearch(quickRecords, event.target.value);
    } else {
      renderDocuments(documents);
    }
    window.SaniKeyUi.showSection("documents", "left");
  });
  advancedInput.addEventListener("input", (event) => {
    setSearchMode("advanced");
    advancedResults.innerHTML = '<p class="muted">Caricamento indice di ricerca avanzata...</p>';
    loadAdvancedSearchData()
      .then((payload) => renderAdvancedResults(payload, event.target.value, clinicalRecords))
      .catch((error) => {
        advancedResults.innerHTML = `<p class="error">${escapeHtml(error.message)}</p>`;
      });
    window.SaniKeyUi.showSection("advanced", "left");
  });
}

try {
  main();
} catch (error) {
  document.body.insertAdjacentHTML("beforeend", `<pre class="error">${error.message}</pre>`);
}
