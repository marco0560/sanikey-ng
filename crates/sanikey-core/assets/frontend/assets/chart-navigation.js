(() => {
"use strict";
const DAY = 86400000;
const DEFAULT_WINDOW_DAYS = 92;
const controllers = new WeakMap();
function domain(datasets) {
  let min = Infinity, max = -Infinity;
  datasets.forEach(dataset => dataset.data.forEach(point => {
    if (Number.isFinite(point.x)) { min = Math.min(min, point.x); max = Math.max(max, point.x); }
  }));
  return Number.isFinite(min) ? {min: min - DAY, max: max + DAY} : {min: 0, max: DAY};
}
function create(canvas, config, windowDays = DEFAULT_WINDOW_DAYS) {
  const original = config.data.datasets.map(dataset => ({...dataset, data: dataset.data.slice()}));
  const bounds = domain(original);
  const windowSpan = Math.max(1, windowDays) * DAY;
  const span = Math.min(windowSpan, bounds.max - bounds.min);
  const travel = bounds.max - bounds.min - span;
  let start = bounds.max - span;
  let chart, frame = 0, disposed = false;
  const controls = document.createElement("div");
  controls.className = "chart-navigation";
  controls.innerHTML = '<div class="chart-navigation-actions"><button type="button" data-pan="first">Prime misure</button><button type="button" data-pan="previous">← Periodo precedente</button><button type="button" data-pan="next">Periodo successivo →</button><button type="button" data-pan="last">Ultime misure</button></div><output class="chart-navigation-range"></output><div class="chart-navigation-scroll" tabindex="0" role="region" aria-label="Scorri la storia del parametro: frecce, Pagina su o giù, Inizio e Fine"><div class="chart-navigation-track"></div></div><div class="chart-navigation-empty" aria-live="polite"></div>';
  canvas.parentElement.after(controls);
  const scroll = controls.querySelector(".chart-navigation-scroll");
  const track = controls.querySelector(".chart-navigation-track");
  const range = controls.querySelector("output");
  const empty = controls.querySelector(".chart-navigation-empty");
  const buttons = [...controls.querySelectorAll("button")];
  function draw() {
    const end = start + span;
    config.options.scales.x.min = start;
    config.options.scales.x.max = end;
    const datasets = original.map(dataset => ({...dataset, data: dataset.data.filter(point => point.x >= start && point.x <= end)}));
    if (chart) { chart.data.datasets = datasets; chart.options.scales.x.min = start; chart.options.scales.x.max = end; chart.update("none"); }
    else { config.data.datasets = datasets; }
    const windowLabel = windowDays <= 45 ? "finestra di circa un mese" : "finestra di circa tre mesi";
    range.textContent = new Date(start).toLocaleDateString("it-IT") + " – " + new Date(end).toLocaleDateString("it-IT") + " · " + windowLabel;
    empty.textContent = datasets.some(dataset => dataset.data.length) ? "" : "Nessuna misurazione in questo periodo.";
    buttons.forEach(button => { button.disabled = ["first", "previous"].includes(button.dataset.pan) ? start <= bounds.min : start >= bounds.max - span; });
  }
  function position() {
    if (!scroll.clientWidth) { return; }
    track.style.width = Math.max(scroll.clientWidth, scroll.clientWidth * (bounds.max - bounds.min) / span) + "px";
    scroll.scrollLeft = travel ? (start - bounds.min) / travel * (scroll.scrollWidth - scroll.clientWidth) : 0;
    scroll.hidden = !travel;
  }
  function move(value) {
    start = Math.max(bounds.min, Math.min(bounds.max - span, value));
    draw(); position();
  }
  function onScroll() {
    if (disposed || !scroll.clientWidth || !travel) { return; }
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (disposed) { return; }
      start = bounds.min + scroll.scrollLeft / Math.max(1, scroll.scrollWidth - scroll.clientWidth) * travel;
      draw();
    });
  }
  function onKey(event) {
    const steps = {ArrowLeft: -span / 4, ArrowRight: span / 4, PageUp: -span, PageDown: span};
    if (event.key in steps || event.key === "Home" || event.key === "End") {
      event.preventDefault();
      move(event.key === "Home" ? bounds.min : event.key === "End" ? bounds.max : start + steps[event.key]);
    }
  }
  buttons.forEach(button => button.addEventListener("click", () => move({first: bounds.min, previous: start - span, next: start + span, last: bounds.max}[button.dataset.pan])));
  scroll.addEventListener("scroll", onScroll);
  scroll.addEventListener("keydown", onKey);
  const observer = new ResizeObserver(position);
  observer.observe(scroll);
  config.plugins = [...(config.plugins || []), {id: "sanikeyTimeWindow", afterDestroy() {
    disposed = true; cancelAnimationFrame(frame); observer.disconnect();
    scroll.removeEventListener("scroll", onScroll); scroll.removeEventListener("keydown", onKey);
    controllers.delete(canvas); controls.remove();
  }}];
  draw();
  chart = new Chart(canvas, config);
  controllers.set(canvas, {reveal(value) {
    if (Number.isFinite(value) && (value < start || value > start + span)) { move(value - span / 2); }
  }});
  position();
  return chart;
}
window.SaniKeyCharts = {create, reveal(canvas, value) { controllers.get(canvas)?.reveal(value); }};
})();
