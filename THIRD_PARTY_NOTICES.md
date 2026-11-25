# Third-party notices

SSH Pro AI bundles the following files in `web/vendor/`:

| Component | Files | License |
|---|---|---|
| [xterm.js](https://github.com/xtermjs/xterm.js) | `xterm.js`, `xterm.css` | MIT, Copyright (c) 2014 The xterm.js authors; (c) 2012-2013 Christopher Jeffrey |
| [@xterm/addon-fit](https://github.com/xtermjs/xterm.js/tree/master/addons/addon-fit) | `addon-fit.js` | MIT, The xterm.js authors |
| [@xterm/addon-web-links](https://github.com/xtermjs/xterm.js/tree/master/addons/addon-web-links) | `addon-web-links.js` | MIT, The xterm.js authors |

The app is built from Rust crates listed in `Cargo.toml` / `Cargo.lock`, each under its own
license (mostly MIT and/or Apache-2.0), among them
[russh](https://github.com/Eugeny/russh) and russh-sftp (Apache-2.0),
[tokio](https://github.com/tokio-rs/tokio) and [axum](https://github.com/tokio-rs/axum) (MIT),
[wry](https://github.com/tauri-apps/wry) and [tao](https://github.com/tauri-apps/tao) (MIT or Apache-2.0),
[rusqlite](https://github.com/rusqlite/rusqlite) (MIT, with SQLite in the public domain) and
[ring](https://github.com/briansmith/ring) (ISC-style). `cargo tree` lists them all.

The installer ships Microsoft's `WebView2Loader.dll` (from the WebView2 SDK, redistributable
under its license) and may download the Microsoft Edge WebView2 Runtime from Microsoft.
