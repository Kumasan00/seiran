//! 実行結果の端末への報告と、終了コードへの変換
//!
//! `main` は本処理の結果をここへ渡し、報告が終わってから `ExitCode` を返す。「何を端末へ出すか」の判断を
//! 出力から分けてあるので、判断そのものは in-src テストで確かめられる。

use std::process::ExitCode;

/// 1 回の実行の終わり方。
pub(super) enum Outcome {
  /// 本処理が成功した。
  Success,
  /// 本処理が失敗した。
  Failure {
    /// ユーザーが最初に読む主診断
    report: miette::Report,
  },
}

impl Outcome {
  /// 端末へ報告し、終了コードを返す。
  ///
  /// 主診断の体裁は miette のグローバル handler（`Report` の `Debug` 表示）に任せ、`Termination` に
  /// 任せていたときと同じ `Error: ` 前置きのまま出す。
  pub(super) fn report(self) -> ExitCode {
    return match self {
      Outcome::Success => ExitCode::SUCCESS,
      Outcome::Failure { report } => {
        eprintln!("Error: {report:?}");
        ExitCode::FAILURE
      },
    };
  }
}
