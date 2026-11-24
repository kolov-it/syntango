//! Sentence in, parses out: the pipeline that ties the parts together.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::lang::Language;
use crate::morph::{Lexicon, LexiconError};
use crate::syntax::lattice::{Edge, Lattice, Morpheme};
use crate::syntax::grammar::Rule;
use crate::syntax::parser::{self, ChartItem, ParseError, Tree};
use crate::syntax::role::Role;
use crate::verb_groups::{VerbGroup, parse_groups};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Lexicon(#[from] LexiconError),
    #[error(transparent)]
    Parse(#[from] ParseError),
}

#[derive(Debug, Serialize)]
pub struct SentenceAnalysis {
    pub tokens: Vec<String>,
    /// Tokens no reading was found for. A sentence with one cannot parse.
    pub unknown: Vec<String>,
    pub lattice: Lattice,
    /// How many parses there are (saturating).
    pub parse_count: u64,
    /// The first parses, at most [`MAX_TREES`].
    pub parses: Vec<Tree>,
    /// Per token, the readings used by at least one parse.
    pub chosen: Vec<TokenChoice>,
    /// Lattice edges any parse uses.
    #[serde(skip)]
    pub used_edges: HashSet<usize>,
    /// For each used edge, the rules that consumed it.
    #[serde(skip)]
    pub edge_rules: HashMap<usize, HashSet<String>>,
    /// The chart items of compound tenses, passives and progressives: the
    /// rules that pass an auxiliary's features up (`aux_mood`, `aux_tense`,
    /// …) or mark a passive (`voice`).
    #[serde(skip)]
    pub verb_items: Vec<ChartItem>,
    /// Per token, its grammatical role in the kept parses.
    pub roles: Vec<TokenRoles>,
    /// Per kept parse (same order as [`SentenceAnalysis::parses`]), its
    /// verbs: every simple verb, compound tense, passive and progressive, each
    /// with its tokens, lemma, cell and voice, in sentence order of the main
    /// verb. *Ho dato il libro a Maria* has one, tokens 0–1, *dare*
    /// `indicative.present_perfect.1s`, active. See [`VerbGroup`].
    pub verb_groups: Vec<Vec<VerbGroup>>,
}

/// Parses kept as trees; the rest are counted, and still count for `chosen`.
pub const MAX_TREES: usize = 20;

#[derive(Debug, Serialize)]
pub struct TokenChoice {
    pub token: String,
    /// Each distinct reading (as its morphemes) any parse used.
    pub readings: Vec<Vec<Morpheme>>,
    /// Every parse read this token the same way.
    pub agreed: bool,
    /// For each reading in `readings` (same order), the grammar rules that
    /// used it across all parses: how the word functions in that reading
    /// (`ap.participle`: a participle used like an adjective;
    /// `v.perfect-essere`: in a compound tense).
    pub uses: Vec<Vec<String>>,
}

/// A token's grammatical role across the parses kept as trees
/// ([`SentenceAnalysis::parses`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenRoles {
    pub token: String,
    /// Each distinct role the token has in a kept parse, the first parse's
    /// first; `None` where a parse gives it no role (a conjunction, a
    /// comma). Empty when the sentence does not parse. A token's role is
    /// that of the innermost phrase with a role holding it: in *Disse che non
    /// riusciva a trovare un lavoro*, `lavoro` is the direct object of
    /// `trovare`.
    pub roles: Vec<Option<Role>>,
    /// Every kept parse gives the token the same role.
    pub agreed: bool,
}

pub async fn analyze(lexicon: &dyn Lexicon, language: &dyn Language, text: &str) -> Result<SentenceAnalysis, Error> {
    let tokens = language.tokenize(text);
    let forms = language.lexicon_forms(&tokens);
    let form_refs: Vec<&str> = forms.iter().map(String::as_str).collect();
    let analyses = lexicon.analyze(language.lang(), &form_refs).await?;
    let lattice = language.lattice(text, &tokens, &analyses);
    let unknown = (0..tokens.len())
        .filter(|t| !lattice.edges.iter().any(|e| e.token == *t))
        .map(|t| tokens[t].clone())
        .collect();
    let forest = parser::parse(language.grammar(), &lattice)?;
    let parse_count = forest.count();
    let (parses, derivations): (Vec<Tree>, Vec<_>) = forest.parses(&lattice, MAX_TREES).into_iter().unzip();
    let used_edges = forest.used_edges();
    let edge_rules = forest.edge_rules();
    let chosen = choices(&tokens, &lattice, &used_edges, &edge_rules, !forest.is_empty());
    let verb_items = forest.items_where(passes_verb_features);
    let roles = token_roles(&tokens, &parses);
    let verb_groups = derivations
        .iter()
        .map(|d| parse_groups(language.grammar(), language.lang(), &lattice, &verb_items, d))
        .collect();
    Ok(SentenceAnalysis { tokens, unknown, lattice, parse_count, parses, chosen, used_edges, edge_rules, verb_items, roles, verb_groups })
}

/// A rule that passes an auxiliary's features up (`aux_mood`, `aux_tense`,
/// …) or marks a passive (`voice`): what [`parse_groups`] reads.
fn passes_verb_features(rule: &Rule) -> bool {
    rule.lhs.constraints.iter().any(|(k, _)| k.starts_with("aux_") || k == "voice")
}

/// A sentence the grammar does not accept as a whole, covered by as few
/// complete phrases as it can be ([`fragments`]).
#[derive(Debug, Serialize)]
pub struct SentenceFragments {
    /// One cheapest cover, in sentence order.
    pub fragments: Vec<Fragment>,
    /// Tokens that cover leaves out: punctuation between fragments, and
    /// words no fragment takes.
    pub left_out: Vec<usize>,
    /// Per token, the readings the cheapest covers use. `agreed` only where
    /// every cheapest cover holds the token in a fragment of two or more
    /// tokens and reads it the same way; a word alone or left out decides
    /// nothing. Weaker evidence than a parse: on the homograph set, 52 of
    /// the 59 labelled words fragments decide agree with the labels (88%),
    /// against 99.4% for parses.
    pub chosen: Vec<TokenChoice>,
}

/// One complete phrase of a fragment cover.
#[derive(Debug, Serialize)]
pub struct Fragment {
    /// A category on the grammar's `fragments` line (`S`, `VP`, `NP`, `PP`).
    pub cat: String,
    /// The tokens `start..end` it spans.
    pub start: usize,
    pub end: usize,
    /// Its first parse. Roles inside it are as in a sentence's parse
    /// ([`Tree::token_roles`]); the fragment itself has none.
    pub tree: Tree,
    /// Its verbs, as [`SentenceAnalysis::verb_groups`] gives a parse's.
    pub verb_groups: Vec<VerbGroup>,
}

/// Covers a sentence that did not parse with the fewest complete phrases of
/// the categories its grammar's `fragments` line names: what the grammar
/// does understand of it. `None` when the sentence parsed or the grammar
/// names no fragments. A second pass over the analysis's lattice, so
/// [`analyze`] stays as fast for callers that want parses only.
pub fn fragments(language: &dyn Language, analysis: &SentenceAnalysis) -> Result<Option<SentenceFragments>, Error> {
    let grammar = language.grammar();
    if analysis.parse_count > 0 || grammar.fragments.is_empty() {
        return Ok(None);
    }
    let lattice = &analysis.lattice;
    let cover = parser::parse_fragments(grammar, lattice)?;
    let verb_items = cover.forest.items_where(passes_verb_features);
    let fragments = cover
        .cover
        .iter()
        .map(|c| {
            let (tree, derivation) = cover.forest.item_parse(c.item, lattice).expect("a complete chart item has a parse");
            Fragment {
                cat: cover.forest.item_cat(c.item).to_owned(),
                start: c.start,
                end: c.end,
                tree,
                verb_groups: parse_groups(grammar, language.lang(), lattice, &verb_items, &derivation),
            }
        })
        .collect();
    let mut chosen = choices(&analysis.tokens, lattice, &cover.forest.used_edges(), &cover.forest.edge_rules(), true);
    for &t in &cover.undecided {
        chosen[t].agreed = false;
    }
    Ok(Some(SentenceFragments { fragments, left_out: cover.left_out, chosen }))
}

fn token_roles(tokens: &[String], parses: &[Tree]) -> Vec<TokenRoles> {
    let per_parse: Vec<Vec<Option<Role>>> = parses.iter().map(|p| p.token_roles(tokens.len())).collect();
    tokens
        .iter()
        .enumerate()
        .map(|(t, token)| {
            let mut roles: Vec<Option<Role>> = Vec::new();
            for parse in &per_parse {
                if !roles.contains(&parse[t]) {
                    roles.push(parse[t]);
                }
            }
            TokenRoles { token: token.clone(), agreed: roles.len() == 1, roles }
        })
        .collect()
}

/// Per token, the readings (paths of morphemes through the token) that the
/// parses use. Built from the edges any parse uses, so it covers every parse,
/// not only the ones kept as trees.
fn choices(
    tokens: &[String],
    lattice: &Lattice,
    used: &HashSet<usize>,
    rules: &HashMap<usize, HashSet<String>>,
    parsed: bool,
) -> Vec<TokenChoice> {
    (0..tokens.len())
        .map(|t| {
            let edges: Vec<(usize, &Edge)> =
                lattice.edges.iter().enumerate().filter(|(e, edge)| edge.token == t && used.contains(e)).collect();
            let starts: HashSet<usize> = edges.iter().map(|(_, e)| e.from).collect();
            let ends: HashSet<usize> = edges.iter().map(|(_, e)| e.to).collect();
            let mut readings: Vec<(usize, Vec<Morpheme>)> = Vec::new();
            for start in starts.difference(&ends) {
                walk(&edges, *start, None, &mut Vec::new(), &mut readings);
            }
            let key = |r: &[Morpheme]| r.iter().map(|m| (m.lemma.clone(), m.cat.clone(), m.feats.clone())).collect::<Vec<_>>();
            let mut unique: Vec<Vec<Morpheme>> = Vec::new();
            let mut uses: Vec<Vec<String>> = Vec::new();
            for (first_edge, r) in readings {
                let i = match unique.iter().position(|u| key(u) == key(&r)) {
                    Some(i) => i,
                    None => {
                        unique.push(r);
                        uses.push(Vec::new());
                        unique.len() - 1
                    }
                };
                uses[i].extend(rules.get(&first_edge).into_iter().flatten().cloned());
                uses[i].sort();
                uses[i].dedup();
            }
            TokenChoice { token: tokens[t].clone(), agreed: parsed && unique.len() == 1, readings: unique, uses }
        })
        .collect()
}

/// Every path of morphemes through a token, with the lattice edge of its
/// first morpheme (the one the grammar's rules consumed as the word).
fn walk(
    edges: &[(usize, &Edge)],
    node: usize,
    first: Option<usize>,
    path: &mut Vec<Morpheme>,
    out: &mut Vec<(usize, Vec<Morpheme>)>,
) {
    let next: Vec<&(usize, &Edge)> = edges.iter().filter(|(_, e)| e.from == node).collect();
    if next.is_empty() {
        if let Some(first) = first {
            out.push((first, path.clone()));
        }
        return;
    }
    for (i, e) in next {
        path.push(e.morph.clone());
        walk(edges, e.to, first.or(Some(*i)), path, out);
        path.pop();
    }
}
