function extendedText(value) {
  return value === null || value === undefined ? "" : String(value);
}

function extendedEscape(value) {
  return extendedText(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function extendedDate(value) {
  const match = extendedText(value).match(/^(\d{4})-(\d{2})-(\d{2})$/);
  return match ? `${match[3]}/${match[2]}/${match[1]}` : extendedText(value);
}

function hasNumericValue(value) {
  return value !== null && value !== undefined && extendedText(value).trim() !== "" && Number.isFinite(Number(value));
}

function extendedOrigin(point) {
  if (point.document_href) {
    return "Documento: " + decodeURIComponent(extendedText(point.document_href).split("/").pop());
  }
  if (point.source_reference) {
    return "Origine: " + point.source_reference;
  }
  return point.source_kind === "curated-observation" ? "Origine: osservazione curata" : "Origine: non disponibile";
}

function extendedPointLabel(raw) {
  if (raw.component) {
    const unit = raw.component.field === "pulse" ? "bpm" : (raw.series.unit || "mmHg");
    return raw.component.label + ": " + raw.point[raw.component.field] + " " + unit;
  }
  const value = raw.point.raw_value || raw.point.value;
  const unit = raw.point.raw_unit || raw.point.normalized_unit || raw.series.unit || "";
  const includesUnit = unit && extendedText(value).trim().toLowerCase().endsWith(unit.toLowerCase());
  return (raw.point.qualifier || "") + value + (unit && !includesUnit ? " " + unit : "");
}

function extendedChartOptions(units) {
  const scales = {
    x: {type: "linear", ticks: {callback: (value) => new Date(Number(value)).toLocaleDateString("it-IT")}},
    y: {type: "linear", position: "left", title: {display: Boolean(units[0]), text: units[0] || "Valore"}},
  };
  if (units[1]) {
    scales.y1 = {type: "linear", position: "right", title: {display: true, text: units[1]}, grid: {drawOnChartArea: false}};
  }
  return {
    responsive: true,
    maintainAspectRatio: false,
    scales,
    plugins: {tooltip: {callbacks: {
      title: (contexts) => contexts.length ? extendedDate(contexts[0].raw.point.date) : "",
      label: (context) => extendedPointLabel(context.raw),
      afterLabel: (context) => extendedOrigin(context.raw.point),
    }}},
    onClick: (_event, elements, chart) => {
      if (!elements.length) { return; }
      const point = chart.data.datasets[elements[0].datasetIndex].data[elements[0].index].point;
      if (point && point.document_href) {
        window.open(point.document_href, "_blank", "noopener");
      }
    },
  };
}

function appendExtendedChart(target, title, datasets, units, windowDays = 92) {
  const card = document.createElement("section");
  card.className = "extended-chart-card";
  card.innerHTML = `<h2>${extendedEscape(title)}</h2>`;
  const canvas = document.createElement("canvas");
  canvas.className = "extended-parameter-chart";
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", title);
  const plot = document.createElement("div");
  plot.className = "extended-chart-plot";
  plot.appendChild(canvas);
  card.appendChild(plot);
  target.appendChild(card);
  window.SaniKeyCharts.create(canvas, {type: "line", data: {datasets}, options: extendedChartOptions(units)}, windowDays);
}

function renderPressureChart(target, series, points) {
  const components = [
    {field: "systolic", label: "Sistolica", color: "#1d4ed8"},
    {field: "diastolic", label: "Diastolica", color: "#dc2626"},
    {field: "pulse", label: "Polso", color: "#15803d"},
  ];
  const datasets = components.map((component) => {
    const data = points.filter((point) => hasNumericValue(point[component.field]))
      .map((point) => ({x: Date.parse(point.date + "T00:00:00"), y: Number(point[component.field]), point, series, component}));
    return data.length ? {label: component.label, data, yAxisID: component.field === "pulse" ? "y1" : "y", borderColor: component.color, backgroundColor: component.color, pointBackgroundColor: component.color, pointBorderColor: "#ffffff", pointBorderWidth: 1.5, pointRadius: 4, pointHoverRadius: 7, pointHitRadius: 8, borderWidth: 2, tension: 0} : null;
  }).filter(Boolean);
  if (datasets.length) {
    const units = [series.unit || "mmHg"];
    if (datasets.some((dataset) => dataset.yAxisID === "y1")) { units.push("bpm"); }
    appendExtendedChart(target, series.name || series.id, datasets, units, 31);
  }
}

function renderNumericCharts(target, entries) {
  if (!entries.length) { return; }
  const byUnit = new Map();
  entries.forEach((entry) => {
    const unit = entry.series.unit || "Senza unita'";
    const group = byUnit.get(unit) || [];
    group.push(entry);
    byUnit.set(unit, group);
  });
  const unitGroups = [...byUnit.entries()];
  const chartGroups = unitGroups.length <= 2 ? [unitGroups] : unitGroups.map((group) => [group]);
  const colors = ["#7c3aed", "#ea580c", "#0f766e", "#be123c", "#0369a1"];
  chartGroups.forEach((chartGroup) => {
    const units = chartGroup.map(([unit]) => unit);
    const datasets = chartGroup.flatMap(([unit, group], unitIndex) => group.map((entry) => {
      const index = entry.styleIndex ?? entries.indexOf(entry);
      const color = index < colors.length ? colors[index] : `hsl(${(index * 137.508) % 360}, 65%, 38%)`;
      return {
      label: entry.series.name || entry.series.id,
      yAxisID: unitIndex ? "y1" : "y",
      data: entry.points.map((point) => ({x: Date.parse(point.date + "T00:00:00"), y: Number(point.numeric_value), point, series: entry.series})),
      borderColor: color,
      backgroundColor: color,
      pointBackgroundColor: color,
      pointBorderColor: "#ffffff",
      pointBorderWidth: 1.5,
      pointRadius: 4,
      pointHoverRadius: 7,
      pointHitRadius: 8,
      pointStyle: ["circle", "rect", "triangle", "rectRot", "crossRot"][index % 5],
      borderDash: index < colors.length ? [] : [4 + index % 4, 3],
      borderWidth: 2,
      tension: 0.15,
      };
    }));
    const title = chartGroup.flatMap(([, group]) => group.map((entry) => entry.series.name || entry.series.id)).join(" · ");
    appendExtendedChart(target, title, datasets, units);
  });
}

function main() {
  const clinical = window.SANIKEY_DATA && window.SANIKEY_DATA.clinical || {};
  const series = (clinical.observation_series || []).slice().sort((left, right) => extendedText(left.name || left.id).localeCompare(extendedText(right.name || right.id), "it"));
  const points = clinical.observation_points || [];
  const requested = new URLSearchParams(window.location.search).get("series");
  const selected = new Set(series.some((item) => item.id === requested) ? [requested] : series.slice(0, 1).map((item) => item.id));
  const seriesTarget = document.querySelector("[data-extended-series]");
  const chartTarget = document.querySelector("[data-extended-charts]");
  const filters = {from: "", to: ""};
  function render() {
    seriesTarget.innerHTML = series.map((item) => `<label><input type="checkbox" value="${extendedEscape(item.id)}" ${selected.has(item.id) ? "checked" : ""}> ${extendedEscape(item.name || item.id)}</label>`).join("");
    seriesTarget.querySelectorAll("input").forEach((input) => input.addEventListener("change", () => {
      input.checked ? selected.add(input.value) : selected.delete(input.value);
      render();
    }));
    chartTarget.querySelectorAll("canvas").forEach((canvas) => Chart.getChart(canvas)?.destroy());
    chartTarget.replaceChildren();
    const entries = series.filter((item) => selected.has(item.id)).map((item) => ({
      series: item,
      styleIndex: series.indexOf(item),
      points: points.filter((point) => point.series_id === item.id && (!filters.from || point.date >= filters.from) && (!filters.to || point.date <= filters.to)),
    }));
    entries.filter((entry) => entry.series.value_type === "blood_pressure").forEach((entry) => renderPressureChart(chartTarget, entry.series, entry.points));
    const numeric = entries.map((entry) => ({...entry, points: entry.points.filter((point) => hasNumericValue(point.numeric_value))})).filter((entry) => entry.series.value_type !== "blood_pressure" && entry.points.length);
    renderNumericCharts(chartTarget, numeric);
    if (!chartTarget.children.length) {
      chartTarget.innerHTML = '<p class="muted">Nessun punto grafico nel periodo selezionato.</p>';
    }
  }
  document.querySelectorAll("[data-extended-filter]").forEach((input) => input.addEventListener("input", () => {
    filters[input.dataset.extendedFilter] = input.value;
    render();
  }));
  render();
}

if (typeof Chart !== "undefined") {
  main();
}
