class SaniKeyMaterialButton extends HTMLElement {
  connectedCallback() {
    if (this.dataset.ready === "true") {
      return;
    }
    this.dataset.ready = "true";
    this.setAttribute("role", "button");
    this.setAttribute("tabindex", this.getAttribute("tabindex") || "0");
    this.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        this.click();
      }
    });
  }
}

["md-filled-button", "md-filled-tonal-button", "md-outlined-button", "md-text-button", "md-icon-button"].forEach((name) => {
  if (!customElements.get(name)) {
    customElements.define(name, class extends SaniKeyMaterialButton {});
  }
});
