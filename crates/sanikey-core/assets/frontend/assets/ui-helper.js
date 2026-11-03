window.SaniKeyUi = (() => {
  const wideLayout = window.matchMedia("(min-width: 72rem)");
  const state = {
    left: "documents",
    right: "timeline",
  };

  function isDualLayout() {
    return wideLayout.matches;
  }

  function fallbackSection(excluded) {
    return ["documents", "timeline", "summary", "therapies", "parameters", "dicom", "advanced"]
      .find((section) => section !== excluded && isSectionAvailable(section)) || "documents";
  }

  function isSectionAvailable(name) {
    const panel = document.querySelector(`[data-section-panel="${name}"]`);
    return Boolean(panel && !panel.hidden);
  }

  function normalizeSection(name) {
    return isSectionAvailable(name) ? name : fallbackSection(name);
  }

  function showSection(name, target = "left") {
    const selected = normalizeSection(name);
    if (selected === "parameters" && isDualLayout()) {
      state.left = "parameters";
      state.right = normalizeSection("parameter-detail");
      applyPanes();
      return;
    }
    if (!isDualLayout() || target !== "right") {
      if (state.right === selected) {
        state.right = fallbackSection(selected);
      }
      state.left = selected;
      applyPanes();
      return;
    }
    if (state.left === selected) {
      state.left = fallbackSection(selected);
    }
    state.right = selected;
    applyPanes();
  }

  function applyPanes() {
    const dual = isDualLayout();
    state.left = normalizeSection(state.left);
    state.right = normalizeSection(state.right);
    if (dual && state.left === "parameters") {
      state.right = normalizeSection("parameter-detail");
    }
    if (dual && state.right === "parameter-detail" && state.left !== "parameters") {
      state.right = fallbackSection(state.left);
    }
    if (state.left === state.right) {
      state.right = fallbackSection(state.left);
    }
    document.body.dataset.layout = dual ? "dual" : "single";
    document.body.dataset.leftPane = state.left;
    document.body.dataset.rightPane = dual ? state.right : "";
    document.querySelectorAll("[data-section-panel]").forEach((panel) => {
      let role = "none";
      if (panel.dataset.sectionPanel === state.left) {
        role = "left";
      } else if (dual && panel.dataset.sectionPanel === state.right) {
        role = "right";
      }
      panel.dataset.paneRole = role;
      panel.classList.toggle("is-active", role !== "none");
    });
    document.querySelectorAll("[data-section-button]").forEach((button) => {
      const selected = button.dataset.sectionButton;
      const target = button.dataset.paneTarget || "left";
      const active = target === "right"
        ? dual && (state.right === selected || (selected === "parameters" && state.left === selected && state.right === "parameter-detail"))
        : state.left === selected;
      button.classList.toggle("is-active", active);
      button.setAttribute("aria-selected", active ? "true" : "false");
    });
    window.dispatchEvent(new CustomEvent("sanikeylayoutchange", {detail: {dual}}));
  }

  function setupSections({defaultSection = "documents", defaultRight = "timeline"} = {}) {
    state.left = normalizeSection(defaultSection === "timeline" ? "documents" : defaultSection);
    state.right = normalizeSection(defaultRight);
    document.querySelectorAll("[data-section-button]").forEach((button) => {
      button.addEventListener("click", () => {
        showSection(button.dataset.sectionButton, button.dataset.paneTarget || "left");
      });
    });
    if (wideLayout.addEventListener) {
      wideLayout.addEventListener("change", applyPanes);
    } else {
      wideLayout.addListener(applyPanes);
    }
    applyPanes();
  }

  return {setupSections, showSection};
})();
