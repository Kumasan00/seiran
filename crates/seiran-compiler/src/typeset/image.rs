//! 画像資源の解決 — 参照パスの収集・読込・自然寸法取得・表示寸法の確定
//!
//! 文書木（HIR）から参照されている画像パスを集め（`manifest`）、`ProjectSource` 経由で読み込んで
//! 自然寸法を確定し、ブロック列の `width` / `height` を埋める（`resources`）までをここに閉じる
//! （#350 で `compiler::{image_manifest, image_resources}` から移設）。
//!
//! 自然寸法の取得（`image` による寸法ヘッダの読み取りと `usvg` による SVG のパース）は
//! 子 module `natural_size` に閉じる（#372 で `seiran_pdf::natural_image_size` から移設）。
//! 描画に使う画像本体のデコード・ダウンサンプリングは render（`seiran-pdf`）の責務。

mod manifest;
mod natural_size;
mod resources;

pub(crate) use manifest::collect_image_paths;
pub(crate) use resources::{ImageResources, load_image_resources, resolve_images};
