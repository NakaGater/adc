//! M2-6 部品コンパイルキャッシュ (docs/binding-cache.md)。
//!
//! キー = sha256(ADCバージョン + Part正準形RON + 参照paramの解決値)。
//! 成果物 = <key>.brep(バイナリBRep)+ <key>.bind.json(束縛表)を併存保存。
//! 束縛表は決定的コンパイルの派生キャッシュであり、正典に幾何IDを持ち込まない
//! (キー不一致=再コンパイルで必ず作り直される)。

use std::collections::BTreeMap;
use std::path::Path;

use adc_kernel::Solid;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{compile_part, CompileError, CompiledPart};
use adc_schema::{Design, EvalContext, Evaluator};

/// anchor → 部分形状インデックス(同一.brepの決定的列挙順に対するローカル整数)
#[derive(Debug, Serialize, Deserialize)]
pub struct BindingTable {
    pub anchors: BTreeMap<String, CachedBinding>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CachedBinding {
    Face { index: usize },
    Edge { index: usize },
    Axis { origin: [f64; 3], dir: [f64; 3] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOutcome {
    Hit,
    Compiled,
}

/// Part正準形RONから `param(<id>)` 参照を収集する(DSL文字列表現に現れる)
fn collect_param_ids(part_ron: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut rest = part_ron;
    while let Some(pos) = rest.find("param(") {
        rest = &rest[pos + 6..];
        if let Some(end) = rest.find(')') {
            let id: String = rest[..end]
                .trim_matches(|c| c == '"' || c == ' ')
                .to_string();
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
            rest = &rest[end..];
        } else {
            break;
        }
    }
    ids.sort();
    ids
}

/// 部品キャッシュキー: hash(ADCバージョン + Part正準形 + 参照param解決値)
pub fn part_cache_key(
    design: &Design,
    part_id: &str,
    ev: &Evaluator,
) -> Result<String, CompileError> {
    let part = design
        .parts
        .iter()
        .find(|p| p.id == part_id)
        .ok_or_else(|| CompileError::Geometry {
            feature_id: part_id.to_string(),
            message: format!("part \"{part_id}\" がDesignに存在しません"),
        })?;
    let part_ron = ron::ser::to_string(part).map_err(|e| CompileError::Geometry {
        feature_id: part_id.to_string(),
        message: format!("正準形シリアライズ失敗: {e}"),
    })?;
    let mut src = format!("adc:{}|{}", env!("CARGO_PKG_VERSION"), part_ron);
    for id in collect_param_ids(&part_ron) {
        src.push_str(&format!("|{id}={:?}", ev.param(&id)));
    }
    let mut h = Sha256::new();
    h.update(src.as_bytes());
    Ok(format!("{:x}", h.finalize()))
}

/// キャッシュ付きコンパイル。ヒット時は.brep+束縛表から復元する。
/// キャッシュ書き込みはベストエフォート(失敗してもコンパイル結果を返す)。
pub fn compile_part_cached(
    design: &Design,
    part_id: &str,
    ctx: &EvalContext,
    cache_dir: &Path,
) -> Result<(CompiledPart, CacheOutcome), CompileError> {
    let ev = Evaluator::new(design, ctx).map_err(CompileError::Eval)?;
    let key = part_cache_key(design, part_id, &ev)?;
    let brep = cache_dir.join(format!("{key}.brep"));
    let bind = cache_dir.join(format!("{key}.bind.json"));

    if brep.exists() && bind.exists() {
        let loaded = (|| -> Result<CompiledPart, String> {
            let solid = Solid::read_brep(brep.to_str().ok_or("パス不正")?)?;
            let table: BindingTable = serde_json::from_str(
                &std::fs::read_to_string(&bind).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            CompiledPart::from_cache(part_id, solid, &table)
        })();
        if let Ok(cp) = loaded {
            return Ok((cp, CacheOutcome::Hit));
        }
        // 破損キャッシュはミス扱い(再コンパイルで上書き)
    }

    let cp = compile_part(design, part_id, ctx)?;
    let _ = std::fs::create_dir_all(cache_dir);
    if let (Some(brep_s), Ok(table)) = (brep.to_str(), cp.binding_table()) {
        let ok = cp.solid.write_brep(brep_s).is_ok();
        if ok {
            let _ = std::fs::write(
                &bind,
                serde_json::to_string(&table).expect("束縛表のシリアライズ"),
            );
        }
    }
    Ok((cp, CacheOutcome::Compiled))
}
