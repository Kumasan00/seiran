//! 組版パイプラインの入口 — phase 内部の段順序（lowering → 計測 → 改行・改ページ等）を集約する
//!
//! 呼び出し側（`seiran_compiler::compiler`）が知るのは「本文／前付け／後付け／走り文をレイアウトする」
//! という 1 操作だけにし、各 phase 内部でどの段をどの順で呼ぶかは本 module に閉じる（issue #281）。

mod back_matter;
mod body;
mod front_matter;

pub use back_matter::{BackMatterInput, layout_back_matter};
pub use body::{BodyLayout, BodyLayoutError, BodyLayoutInput, layout_body};
pub use front_matter::{FrontMatterInput, layout_front_matter};
