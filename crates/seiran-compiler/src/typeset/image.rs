//! 画像資源の解決 — 参照パスの収集・読込・自然寸法取得・表示寸法の確定
//!
//! 文書木（HIR）から参照されている画像パスを集め（`manifest`）、`ProjectSource` 経由で読み込んで
//! 自然寸法を確定し、ブロック列の `width` / `height` を埋める（`resources`）までをここに閉じる
//! （#350 で `compiler::{image_manifest, image_resources}` から移設）。
//!
//! 自然寸法の取得だけは描画側 crate の `seiran_pdf::natural_image_size`（krilla / usvg による
//! デコード）を呼ぶ。`typeset` が知ってよい `seiran_pdf` はこの画像デコードの leaf 関数までで、
//! 描画 API（`Publication` / `render`）には触れない。

mod manifest;
mod resources;

pub(crate) use manifest::collect_image_paths;
pub(crate) use resources::{ImageResources, load_image_resources, resolve_images};
