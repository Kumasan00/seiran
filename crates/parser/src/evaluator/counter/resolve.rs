//! pass2: `\ref` 解決 — `DocNode` ツリーを歩いて `InlineNode::Ref` を埋める

use document::{DocNode, InlineNode, ListItem, ProofTarget};

use super::CounterRegistry;
use crate::evaluator::EvalError;

/// `Vec<DocNode>` を再帰的に走査して `InlineNode::Ref` を `CounterRegistry` で解決する
///
/// pass1（`Evaluator::evaluate_children`）で生成された `Ref { number: None }` を
/// `registry.resolve_label(label)` で書き換える。未登録ラベルは
/// [`EvalError::UnknownLabel`] を返す。
///
/// # Errors
///
/// 未定義ラベルが見つかった場合に [`EvalError::UnknownLabel`] を返します。
pub(crate) fn resolve_refs(nodes: &mut [DocNode], registry: &CounterRegistry) -> Result<(), EvalError> {
  for node in nodes {
    match node {
      DocNode::Heading { title: inlines, .. }
      | DocNode::Paragraph(inlines)
      | DocNode::Figure {
        caption: Some(inlines),
        ..
      } => resolve_inlines(inlines, registry)?,
      DocNode::List { items, .. } => {
        for item in items {
          resolve_list_item(item, registry)?;
        }
      },
      DocNode::Theorem { body, of, .. } => {
        resolve_refs(body, registry)?;
        if let Some(target) = of {
          resolve_proof_target(target, registry)?;
        }
      },
      // 引用本体は通常の本文と同じく `\ref` を含みうるため再帰する
      DocNode::Quote { body, .. } => resolve_refs(body, registry)?,
      DocNode::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter_mut().chain(rows.iter_mut()) {
          for cell in &mut row.cells {
            resolve_inlines(&mut cell.content, registry)?;
          }
        }
        if let Some(inlines) = caption {
          resolve_inlines(inlines, registry)?;
        }
      },
      // 数式中の `\ref` は対象外（現状の MathNode に Ref バリアントがないため）。
      // `DocNode::Anchor` は CSL 整形ステージが parser の後に追加するため、ここには届かない。
      DocNode::MathBlock { .. }
      | DocNode::Figure { caption: None, .. }
      | DocNode::Rule { .. }
      | DocNode::PageBreak
      | DocNode::Space(_)
      | DocNode::Anchor(_) => {},
    }
  }
  return Ok(());
}

fn resolve_list_item(item: &mut ListItem, registry: &CounterRegistry) -> Result<(), EvalError> {
  return resolve_refs(&mut item.content, registry);
}

/// `proof` の `[of=label]` 参照を解決する（`\ref` 解決と同型）
///
/// 既に解決済み（`number.is_some()`）なら何もしない。未登録ラベルは
/// [`EvalError::UnknownLabel`] を返す。
fn resolve_proof_target(target: &mut ProofTarget, registry: &CounterRegistry) -> Result<(), EvalError> {
  if target.number.is_some() {
    return Ok(());
  }
  let Some(resolved) = registry.resolve_label(&target.label) else {
    return Err(EvalError::UnknownLabel {
      label: target.label.clone(),
      span: target.span,
    });
  };
  target.number = Some(resolved.to_string());
  return Ok(());
}

fn resolve_inlines(inlines: &mut [InlineNode], registry: &CounterRegistry) -> Result<(), EvalError> {
  for inline in inlines {
    match inline {
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. }
      | InlineNode::InternalLink { children, .. } => {
        resolve_inlines(children, registry)?;
      },
      InlineNode::Ref {
        label,
        number,
        span,
      } => {
        if number.is_some() {
          continue;
        }
        let Some(resolved) = registry.resolve_label(label) else {
          return Err(EvalError::UnknownLabel {
            label: label.clone(),
            span: *span,
          });
        };
        *number = Some(resolved.to_string());
      },
      // `\cite` のキー検証は cite::resolve_cites が別途行う（ここでは触らない）
      InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::NoIndent
      | InlineNode::Cite { .. } => {},
    }
  }
  return Ok(());
}
