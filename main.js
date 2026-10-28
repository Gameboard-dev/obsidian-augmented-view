// Augmented View. The plugin is Rust compiled to augmented_view.wasm (core/); this file only hands it Obsidian. The .wasm
// carries its own JavaScript bindings in a custom section, "glue" (core/pack), so this is the only JavaScript file.
const obsidian = require("obsidian");
// A release has the module here, base64-encoded (pack: Obsidian installs only main.js, manifest.json and styles.css).
const WASM = null;

module.exports = class AugmentedView extends obsidian.Plugin {
  async onload() {
    const bytes = WASM
      ? Uint8Array.from(atob(WASM), (c) => c.charCodeAt(0))
      : await this.app.vault.adapter.readBinary(`${this.manifest.dir}/augmented_view.wasm`);
    const module = await WebAssembly.compile(bytes);
    const glue = new TextDecoder().decode(WebAssembly.Module.customSections(module, "glue")[0]);
    const rust = new Function(`${glue}\nreturn wasm_bindgen;`)();
    rust.initSync({ module });
    this.rust = rust.start(this, obsidian, require, module, glue);
  }

  onunload() {
    this.rust?.unload();
  }
};
