//! 画像資源の解決 — 参照パスの収集・読込・自然寸法取得・表示寸法の確定
//!
//! 文書木（HIR）から参照されている画像ファイルと PDF のページを集め（`manifest`）、`ProjectSource` 経由で
//! 読み込んで自然寸法を検証し、図 1 件ぶんの描画寸法を確定する（`resources`）までをここに閉じる。
//! 図の単位は（パス, PDF のページ index）で、バイト列はファイルごとに 1 つ。
//!
//! 自然寸法の取得は、ラスタ（`image` による寸法ヘッダの読み取り）と SVG（`usvg` のパース）が子 module
//! `natural_size`、PDF（`hayro-syntax` による検査とページの選択）が子 module `pdf` に閉じる。

mod manifest;
mod natural_size;
mod pdf;
mod resources;

pub(super) use manifest::collect_image_requests;
pub(super) use resources::{ImageAsset, ImageResources, load_image_resources, pdf_page_index, resolve_image_size};
