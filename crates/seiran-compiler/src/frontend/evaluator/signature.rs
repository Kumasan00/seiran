//! 名前で引く呼び出し（コマンド・環境・環境内コマンド・数式コマンド）の署名
//!
//! 署名は必須引数の位置ごとの読み取り方・不足時の説明・受け付けるキーを 1 つの値で宣言する。必須引数の個数と未知キーの
//! 検査は dispatch がこの宣言から [`Signature::bind`] で行い、ハンドラは検査を呼ばない。宣言を持たない呼び出し（記号
//! コマンド等）は [`Signature::NONE`] で引数もキーも拒否へ倒れる。
//!
//! 検査済みの必須引数は [`Bound::args`] の `[_; N]` で渡るので、ハンドラが読める個数は宣言した個数に型で一致する。

use crate::frontend::{
  evaluator::{
    EvalError,
    opt_args::{self, OptArgs, OptDecl},
  },
  syntax::{
    ArgMode, CstNode,
    view::{CommandView, EnvironmentView},
  },
};

/// 名前で引く呼び出しの署名（`N` は必須引数の個数）
#[derive(Debug, Clone, Copy)]
pub(super) struct Signature<const N: usize> {
  /// 必須引数の読み取り方（位置順）。parser へ渡す読み取り方と個数の両方がここから引かれる
  modes: [ArgMode; N],
  /// 必須引数が足りないときの診断に載せる説明（「ラベル名」「2 個（分子と分母）」等。`N = 0` では読まれない）
  expected: &'static str,
  /// 任意引数 `[...]` の扱い
  options: Options,
}

/// 任意引数 `[...]` の扱い
#[derive(Debug, Clone, Copy)]
pub(super) enum Options {
  /// 名前付きキーとして検査する（空なら任意引数を受け付けない）
  Keys(&'static [OptDecl]),
  /// `\sqrt` の根指数 — 設定オプションではなく数式内容なので、キーの検査を通さない
  MathIndex,
}

/// 署名で検査済みの必須引数と任意引数
#[derive(Debug)]
pub(super) struct Bound<'a, const N: usize> {
  /// 必須引数（位置順、宣言した個数ちょうど）
  pub(super) args: [&'a CstNode<'a>; N],
  /// 宣言したキーで検査済みの任意引数（[`Options::MathIndex`] では空）
  pub(super) opts: OptArgs,
}

impl<const N: usize> Signature<N> {
  /// 必須引数の読み取り方・不足時の説明・受け付けるキーから署名を作る
  pub(super) const fn new(modes: [ArgMode; N], expected: &'static str, keys: &'static [OptDecl]) -> Self {
    return Self {
      modes,
      expected,
      options: Options::Keys(keys),
    };
  }

  /// 任意引数を名前付きキーとして検査するか（根指数を読む署名だけが `false`）
  #[cfg(test)]
  pub(super) fn reads_keys(&self) -> bool { return matches!(self.options, Options::Keys(_)); }

  /// 位置 `index`（0 始まり）の必須引数の読み取り方（宣言の範囲外は外側文脈の継承）
  pub(super) fn arg_mode(&self, index: usize) -> ArgMode {
    return self.modes.get(index).copied().unwrap_or(ArgMode::Inherit);
  }

  /// コマンド呼び出しを署名で検査し、必須引数と任意引数を取り出す
  ///
  /// キー → 個数の順に検査する。
  ///
  /// # Errors
  ///
  /// 任意引数の検査（[`opt_args::collect_opt_args`]）のエラー、必須引数が足りない場合に
  /// [`EvalError::MissingCommandArgument`]、多い場合に [`EvalError::ExtraCommandArgument`] を返します。
  pub(super) fn bind<'a>(&self, view: &CommandView<'a>) -> Result<Bound<'a, N>, EvalError> {
    let opts = match self.options {
      Options::Keys(keys) => opt_args::collect_opt_args(view.source(), view.name(), view.opt_arg(), keys)?,
      Options::MathIndex => OptArgs::default(),
    };
    let args: Vec<&'a CstNode<'a>> = view.args().collect();
    return match <[&'a CstNode<'a>; N]>::try_from(args) {
      Ok(args) => Ok(Bound { args, opts }),
      Err(args) if args.len() < N => Err(EvalError::MissingCommandArgument {
        name: view.name().to_string(),
        expected: self.expected.to_string(),
        span: view.span().into(),
      }),
      Err(_) => Err(EvalError::ExtraCommandArgument {
        name: view.name().to_string(),
        span: view.span().into(),
      }),
    };
  }
}

impl Signature<1> {
  /// 任意引数を根指数（数式内容）として読む署名を作る（`\sqrt` 専用）
  pub(super) const fn math_index(modes: [ArgMode; 1], expected: &'static str) -> Self {
    return Self {
      modes,
      expected,
      options: Options::MathIndex,
    };
  }
}

impl Signature<0> {
  /// 必須引数も任意引数も受け付けない署名（宣言を持たない呼び出しの既定）
  pub(super) const NONE: Self = Self::keys_only(&[]);

  /// 必須引数を取らず、`keys` だけを受け付ける署名を作る
  pub(super) const fn keys_only(keys: &'static [OptDecl]) -> Self { return Self::new([], "", keys); }

  /// 環境を署名で検査し、任意引数を取り出す
  ///
  /// 環境名の必須引数（`\begin{name}` の `{name}`）はビューが除いているので、ここで見るのは著者が余分に書いた引数だけ。
  ///
  /// # Errors
  ///
  /// 任意引数の検査（[`opt_args::collect_opt_args`]）のエラーと、必須引数がある場合に
  /// [`EvalError::ExtraEnvironmentArgument`] を返します。
  #[cfg_attr(not(test), expect(dead_code, reason = "TEMP(#1145): 環境の移行 が使い始める"))]
  pub(super) fn bind_environment(&self, view: &EnvironmentView<'_>) -> Result<OptArgs, EvalError> {
    let opts = match self.options {
      Options::Keys(keys) => opt_args::collect_opt_args(view.source(), view.name(), view.opt_arg(), keys)?,
      Options::MathIndex => {
        unreachable!("根指数の署名は `Signature<1>::math_index` でしか作れず、必須引数 0 個の環境の署名には現れない")
      },
    };
    if view.has_args() {
      return Err(EvalError::ExtraEnvironmentArgument {
        name: view.name().to_string(),
        span: view.span().into(),
      });
    }
    return Ok(opts);
  }
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::{
    evaluator::{
      opt_args::{self, OptDecl, OptKey},
      test_support,
    },
    syntax::{CstElement, CstNode, SyntaxKind},
  };

  /// `[label=...]`
  const LABEL: OptKey<String> = opt_args::string("label");
  /// `LABEL` だけを受け付けるキー列
  const LABEL_KEYS: &[OptDecl] = &[LABEL.decl()];

  /// スニペットを CST へ構文解析し、最初の `Environment` ノードを取り出す
  fn environment_node<'a>(source: &'a str, arena: &'a Bump) -> &'a CstNode<'a> {
    let cst = test_support::parse_cst(source, arena).unwrap();
    for child in cst.children {
      if let CstElement::Node(node) = child
        && node.kind == SyntaxKind::Environment
      {
        return node;
      }
    }
    panic!("Environment ノードが見つかりません");
  }

  #[test]
  fn bind_returns_the_declared_arguments() {
    let arena = Bump::new();
    let source = r"\ref{a}";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let signature = Signature::new([ArgMode::Inherit], "ラベル名", &[]);

    let bound = signature.bind(&view).unwrap();

    assert_eq!(&source[bound.args[0].span.start as usize..bound.args[0].span.end as usize], "{a}");
  }

  #[test]
  fn bind_reports_missing_with_the_expectation() {
    let arena = Bump::new();
    let source = r"\ref";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let signature = Signature::new([ArgMode::Inherit], "ラベル名", &[]);

    let result = signature.bind(&view);

    assert!(
      matches!(result, Err(EvalError::MissingCommandArgument { ref name, ref expected, .. })
        if name == "ref" && expected == "ラベル名"),
      "{result:?}"
    );
  }

  #[test]
  fn bind_reports_extra() {
    let arena = Bump::new();
    let source = r"\href{a}{b}{c}";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let signature = Signature::new([ArgMode::Verbatim, ArgMode::Inherit], "2 個", &[]);

    let result = signature.bind(&view);

    assert!(
      matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "href"),
      "{result:?}"
    );
  }

  #[test]
  fn bind_checks_keys_before_the_count() {
    let arena = Bump::new();
    let source = r"\section[x=1]";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let signature = Signature::new([ArgMode::Inherit], "セクション名", LABEL_KEYS);

    let result = signature.bind(&view);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "x"), "{result:?}");
  }

  #[test]
  fn bind_returns_the_declared_keys() {
    let arena = Bump::new();
    let source = r"\section[label=a]{見出し}";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let signature = Signature::new([ArgMode::Inherit], "セクション名", LABEL_KEYS);

    let bound = signature.bind(&view).unwrap();

    assert_eq!(bound.opts.get(LABEL), Some("a".to_string()));
  }

  #[test]
  fn none_rejects_an_argument_and_a_key() {
    // 宣言の無い呼び出しは引数もキーも受け付けない
    let arena = Bump::new();
    let with_arg = r"\x{a}";
    let with_key = r"\x[k=v]";
    let arg_view = CommandView::new(test_support::command_call_node(with_arg, &arena), with_arg);
    let key_view = CommandView::new(test_support::command_call_node(with_key, &arena), with_key);

    let arg_result = Signature::NONE.bind(&arg_view);
    let key_result = Signature::NONE.bind(&key_view);

    assert!(
      matches!(arg_result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "x"),
      "{arg_result:?}"
    );
    assert!(
      matches!(key_result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"),
      "{key_result:?}"
    );
  }

  #[test]
  fn math_index_does_not_read_keys() {
    let arena = Bump::new();
    let source = r"$\sqrt[3]{x}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let Some(CstElement::Node(math)) = cst.children.first() else {
      panic!("インライン数式が先頭に来るはず");
    };
    let Some(command) = math.children.iter().find_map(|child| {
      return match child {
        CstElement::Node(node) if node.kind == SyntaxKind::CommandCall => Some(*node),
        CstElement::Node(_) | CstElement::Token(_) => None,
      };
    }) else {
      panic!("数式の中に \\sqrt があるはず");
    };
    let view = CommandView::new(command, source);
    let signature = Signature::math_index([ArgMode::Inherit], "1 個（被開平数）");

    let bound = signature.bind(&view).unwrap();

    assert_eq!(bound.opts.get(LABEL), None);
  }

  #[test]
  fn arg_mode_beyond_the_declaration_is_inherit() {
    let signature = Signature::new([ArgMode::Verbatim], "URI", &[]);

    assert_eq!(signature.arg_mode(0), ArgMode::Verbatim);
    assert_eq!(signature.arg_mode(1), ArgMode::Inherit);
  }

  #[test]
  fn bind_environment_rejects_an_argument_and_a_key() {
    let arena = Bump::new();
    let with_arg = r"\begin{quote}{x}\end{quote}";
    let with_key = r"\begin{quote}[k=v]\end{quote}";
    let arg_view = EnvironmentView::new(environment_node(with_arg, &arena), with_arg);
    let key_view = EnvironmentView::new(environment_node(with_key, &arena), with_key);

    let arg_result = Signature::keys_only(&[]).bind_environment(&arg_view);
    let key_result = Signature::keys_only(&[]).bind_environment(&key_view);

    assert!(
      matches!(arg_result, Err(EvalError::ExtraEnvironmentArgument { ref name, .. }) if name == "quote"),
      "{arg_result:?}"
    );
    assert!(
      matches!(key_result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"),
      "{key_result:?}"
    );
  }
}
