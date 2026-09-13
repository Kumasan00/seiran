//! CLI が開く工程（`render` / `write`）の開始と、結果付きの終了の記録（#551）
//!
//! compiler の工程は `seiran-compiler` の同名 module が記録する。メッセージ（「工程を開始」「工程を終了」）と
//! フィールド（`status` / `elapsed`）は 2 つの module で同一に保つ — ログを読む側から見て 1 つの契約だから。
//! 型を共有しないのは、event の target を各 crate に保つため（`RUST_LOG=seiran=info` で CLI の工程だけを
//! 見る絞り込みを効かせる）。
//!
//! 終了の状態は「[`Phase::succeed`] が呼ばれたか」だけで決まり、`?` の早期 return・panic の unwind は
//! `status=Failed` として記録される。

use std::time::Instant;

use tracing::{Span, info, span::EnteredSpan};

/// 工程の終わり方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhaseStatus {
  /// 工程が成果を返した
  Succeeded,
  /// 工程が成果を返さずに終わった（失敗の返却・panic）
  Failed,
}

/// 1 工程の記録。生成で開始を、drop で終了を記録する。
#[must_use = "drop した時点で工程の終了を記録するので、工程の処理が終わるまで束縛しておく"]
pub(super) struct Phase {
  /// 工程の span（drop で抜ける）。値を読まず保持するだけなので `_` 始まり
  _span: EnteredSpan,
  /// 開始時刻
  started: Instant,
  /// 現時点での終わり方（[`Phase::succeed`] まで `Failed`）
  status: PhaseStatus,
}

impl Phase {
  /// `span` に入り、工程の開始を記録する。
  ///
  /// span は呼び出し側の `info_span!("render")` 等で作る（名前と target は callsite で決まる）。
  pub(super) fn enter(span: Span) -> Self {
    let span = span.entered();
    info!("工程を開始");
    return Phase {
      _span: span,
      started: Instant::now(),
      status: PhaseStatus::Failed,
    };
  }

  /// 工程が成果を返したことを記録し、終了する。
  pub(super) fn succeed(mut self) { self.status = PhaseStatus::Succeeded; }
}

impl Drop for Phase {
  /// 工程の終了を記録する。
  ///
  /// `drop` の本体はフィールド（`_span`）の drop より先に走るので、終了 event は工程の span の中で出る。
  fn drop(&mut self) {
    info!(status = ?self.status, elapsed = ?self.started.elapsed(), "工程を終了");
  }
}
