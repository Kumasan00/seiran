//! 工程（phase）の開始と、結果付きの終了の記録（#551）
//!
//! 工程の入れ子は span が表す（#500）。この module は、その span の中で「工程を開始」と
//! 「工程を終了」（`status` と `elapsed`）の 2 つの INFO event を出す唯一の site を持つ。事実（件数）は
//! 引き続き各工程の callee が完了 event で出し、所要時間は終了 event だけが持つ（1 事象 1 オーナー）。
//!
//! 終了の状態は「[`Phase::succeed`] が呼ばれたか」だけで決まる。`?` の早期 return・失敗を返す
//! `return`・panic の unwind はどれも `succeed` を通らないので、失敗した工程も必ず終了 event を持つ
//! — ログから「どの工程で止まったか」を読めるようにするのがこの記録の目的だから。
//!
//! CLI の `render` / `write` は `seiran` crate の同名 module が同じ契約で記録する（target を各 crate に
//! 保つため、この型を facade へは載せない）。

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
///
/// 工程の span を保持するので、この値が生きている間の event はすべて工程の prefix を持つ。
#[must_use = "drop した時点で工程の終了を記録するので、工程の処理が終わるまで束縛しておく"]
pub(crate) struct Phase {
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
  /// span は呼び出し側の `info_span!("input")` 等で作る — span の名前と target は callsite で決まり、
  /// ここで作ると全工程の target がこの module になってしまう。
  pub(crate) fn enter(span: Span) -> Self {
    let span = span.entered();
    info!("工程を開始");
    return Phase {
      _span: span,
      started: Instant::now(),
      status: PhaseStatus::Failed,
    };
  }

  /// 工程が成果を返したことを記録し、終了する。
  pub(crate) fn succeed(mut self) { self.status = PhaseStatus::Succeeded; }
}

impl Drop for Phase {
  /// 工程の終了を記録する。
  ///
  /// `drop` の本体はフィールド（`_span`）の drop より先に走るので、終了 event は工程の span の中で出る。
  fn drop(&mut self) {
    info!(status = ?self.status, elapsed = ?self.started.elapsed(), "工程を終了");
  }
}
