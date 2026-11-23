#![cfg(feature = "wordholder")]
//! Each kept parse's verb groups (`SentenceAnalysis::verb_groups`), against
//! the recorded answers for the homograph sets (tests/fixtures/<lang>-set.json),
//! patched as the command line uses them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syntango::lang;
use syntango::morph::Lang;
use syntango::patch::PatchedLexicon;
use syntango::syntax::parser::Tree;
use syntango::verbs::verb_use;
use syntango::wordholder::{self, FixtureLexicon};
use syntango::{Role, SentenceAnalysis, VerbGroup, Voice, analyze};

async fn run(lang: Lang, text: &str) -> SentenceAnalysis {
    let fixture = FixtureLexicon::load(Path::new(&format!(
        "tests/fixtures/{}-set.json",
        lang.code()
    )))
    .unwrap();
    let lexicon = PatchedLexicon::new(Box::new(fixture), wordholder::patches().unwrap());
    let language = lang::load(lang).unwrap();
    let a = analyze(&lexicon, language.as_ref(), text).await.unwrap();
    assert!(!a.parses.is_empty(), "{text}: no parse");
    assert_eq!(
        a.verb_groups.len(),
        a.parses.len(),
        "{text}: one list per kept parse"
    );
    a
}

/// What a test expects of a group.
struct Expected<'a> {
    tokens: Vec<usize>,
    auxiliaries: Vec<usize>,
    participle: Option<usize>,
    verb: usize,
    lemma: &'a str,
    cell: Option<&'a str>,
    voice: Voice,
    progressive: bool,
}

fn check(g: &VerbGroup, e: &Expected) {
    assert_eq!(g.tokens, e.tokens, "{g:?}");
    assert_eq!(g.auxiliaries, e.auxiliaries, "{g:?}");
    assert_eq!(g.participle, e.participle, "{g:?}");
    assert_eq!(g.verb, e.verb, "{g:?}");
    assert_eq!(g.lemma, e.lemma, "{g:?}");
    assert_eq!(g.cell.as_deref(), e.cell, "{g:?}");
    assert_eq!(g.voice, e.voice, "{g:?}");
    assert_eq!(g.progressive, e.progressive, "{g:?}");
}

fn simple<'a>(token: usize, lemma: &'a str, cell: &'a str) -> Expected<'a> {
    Expected {
        tokens: vec![token],
        auxiliaries: Vec::new(),
        participle: None,
        verb: token,
        lemma,
        cell: Some(cell),
        voice: Voice::Active,
        progressive: false,
    }
}

/// Every kept parse has exactly these groups.
fn every_parse(a: &SentenceAnalysis, expected: &[Expected]) {
    for (p, groups) in a.verb_groups.iter().enumerate() {
        assert_eq!(groups.len(), expected.len(), "parse {p}: {groups:?}");
        for (g, e) in groups.iter().zip(expected) {
            check(g, e);
        }
    }
}

#[tokio::test]
async fn italian_compound_tense_is_one_group_named_after_the_auxiliary() {
    // Two parses: a Roma is where, or to whom (#53 left dare one).
    let a = run(Lang::It, "Ho scritto una lettera a Roma.").await;
    assert!(a.parses.len() > 1, "the test is about every parse");
    every_parse(
        &a,
        &[Expected {
            tokens: vec![0, 1],
            auxiliaries: vec![0],
            participle: Some(1),
            verb: 1,
            lemma: "scrivere",
            cell: Some("indicative.present_perfect.1s"),
            voice: Voice::Active,
            progressive: false,
        }],
    );
}

#[tokio::test]
async fn italian_simple_verb_is_a_one_token_group() {
    let a = run(Lang::It, "La ragazza ama il cane.").await;
    every_parse(&a, &[simple(2, "amare", "indicative.present.3s")]);
}

/// Issue #48: in caso + a clause is subordinate, so its present subjunctive
/// in the 2nd person stays.
#[tokio::test]
async fn italian_in_caso_takes_the_subjunctive() {
    let a = run(
        Lang::It,
        "In caso tu venga licenziato, cosa faresti per prima cosa?",
    )
    .await;
    every_parse(
        &a,
        &[
            Expected {
                tokens: vec![3, 4],
                auxiliaries: vec![3],
                participle: Some(4),
                verb: 4,
                lemma: "licenziare",
                cell: Some("subjunctive.present.2s"),
                voice: Voice::Passive,
                progressive: false,
            },
            simple(7, "fare", "conditional.present.2s"),
        ],
    );
}

/// Issue #48: a main clause passato prossimo is neither a subjunctive nor a
/// 1st person imperative ("let's have arrived").
#[tokio::test]
async fn italian_main_clause_passato_prossimo_is_indicative() {
    let a = run(Lang::It, "Siamo arrivati.").await;
    every_parse(
        &a,
        &[Expected {
            tokens: vec![0, 1],
            auxiliaries: vec![0],
            participle: Some(1),
            verb: 1,
            lemma: "arrivare",
            cell: Some("indicative.present_perfect.1p"),
            voice: Voice::Active,
            progressive: false,
        }],
    );
}

#[tokio::test]
async fn italian_compound_passive_has_both_auxiliaries() {
    let a = run(Lang::It, "Il libro è stato letto.").await;
    let [g] = a.verb_groups[0].as_slice() else {
        panic!("{:?}", a.verb_groups[0])
    };
    check(
        g,
        &Expected {
            tokens: vec![2, 3, 4],
            auxiliaries: vec![2, 3],
            participle: Some(4),
            verb: 4,
            lemma: "leggere",
            cell: Some("indicative.present_perfect.3s"),
            voice: Voice::Passive,
            progressive: false,
        },
    );
    // Another parse reads letto as an adjective: è stato is then essere's
    // own present perfect.
    assert!(
        a.verb_groups.iter().flatten().any(|g| g.lemma == "essere"
            && g.tokens == [2, 3]
            && g.cell.as_deref() == Some("indicative.present_perfect.3s")
            && g.voice == Voice::Active),
        "{:?}",
        a.verb_groups
    );
}

#[tokio::test]
async fn italian_passive_of_a_simple_tense_takes_the_auxiliary_cell() {
    let a = run(Lang::It, "Il libro fu letto.").await;
    let [g] = a.verb_groups[0].as_slice() else {
        panic!("{:?}", a.verb_groups[0])
    };
    check(
        g,
        &Expected {
            tokens: vec![2, 3],
            auxiliaries: vec![2],
            participle: Some(3),
            verb: 3,
            lemma: "leggere",
            cell: Some("indicative.past_historic.3s"),
            voice: Voice::Passive,
            progressive: false,
        },
    );
}

#[tokio::test]
async fn a_word_between_auxiliary_and_participle_is_not_in_the_group() {
    let a = run(Lang::It, "Avete mai letto una poesia cinese?").await;
    every_parse(
        &a,
        &[Expected {
            tokens: vec![0, 2],
            auxiliaries: vec![0],
            participle: Some(2),
            verb: 2,
            lemma: "leggere",
            cell: Some("indicative.present_perfect.2p"),
            voice: Voice::Active,
            progressive: false,
        }],
    );
}

#[tokio::test]
async fn coordinated_participles_share_their_auxiliary() {
    let a = run(Lang::It, "Sono nato e cresciuto in campagna.").await;
    let groups = &a.verb_groups[0];
    let shared = |verb: usize, lemma: &'static str| Expected {
        tokens: vec![0, verb],
        auxiliaries: vec![0],
        participle: Some(verb),
        verb,
        lemma,
        cell: Some("indicative.present_perfect.1s"),
        voice: Voice::Active,
        progressive: false,
    };
    let [nato, cresciuto] = groups.as_slice() else {
        panic!("{groups:?}")
    };
    check(nato, &shared(1, "nascere"));
    check(cresciuto, &shared(3, "crescere"));
}

#[tokio::test]
async fn italian_progressive_is_the_auxiliary_and_the_gerund() {
    let a = run(Lang::It, "I prezzi stanno crescendo.").await;
    let [g] = a.verb_groups[0].as_slice() else {
        panic!("{:?}", a.verb_groups[0])
    };
    check(
        g,
        &Expected {
            tokens: vec![2, 3],
            auxiliaries: vec![2],
            participle: None,
            verb: 3,
            lemma: "crescere",
            cell: Some("indicative.present.3p"),
            voice: Voice::Active,
            progressive: true,
        },
    );
}

/// Each kept parse's groups as "lemma cell tokens", one string per parse.
fn group_lists(a: &SentenceAnalysis) -> BTreeSet<String> {
    a.verb_groups
        .iter()
        .map(|groups| {
            groups
                .iter()
                .map(|g| {
                    format!(
                        "{} {} {:?}",
                        g.lemma,
                        g.cell.as_deref().unwrap_or("-"),
                        g.tokens
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .collect()
}

/// Issue #49: after essere, the participle of a verb that takes essere is the
/// compound tense, neither an adjective nor a bare singular noun spelled like
/// it (venuto, stato, successo). `suonare` is the lexicon's reading of sono
/// (out of scope here): it stays a parse of its own.
#[tokio::test]
async fn essere_participle_after_essere_is_the_compound_tense() {
    let cases: [(&str, &[&str]); 6] = [
        (
            "Sono venuto ieri.",
            &[
                "venire indicative.present_perfect.1s [0, 1]",
                "suonare indicative.present.1s [0]",
            ],
        ),
        (
            "Sono venuti.",
            &[
                "venire indicative.present_perfect.3p [0, 1]",
                "suonare indicative.present.1s [0]",
                // A bare plural noun can be a predicate (they are newcomers).
                "essere indicative.present.1s [0]",
                "essere indicative.present.3p [0]",
            ],
        ),
        (
            "Sono stato a Roma.",
            &[
                "essere indicative.present_perfect.1s [0, 1]",
                "stare indicative.present_perfect.1s [0, 1]",
                "suonare indicative.present.1s [0]",
            ],
        ),
        (
            "Siamo arrivati.",
            &["arrivare indicative.present_perfect.1p [0, 1]"],
        ),
        (
            "È successo.",
            &["succedere indicative.present_perfect.3s [0, 1]"],
        ),
        (
            "Sono nato e cresciuto in campagna.",
            &[
                "nascere indicative.present_perfect.1s [0, 1]; crescere indicative.present_perfect.1s [0, 3]",
            ],
        ),
    ];
    for (text, expected) in cases {
        let a = run(Lang::It, text).await;
        let expected: BTreeSet<String> = expected.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(group_lists(&a), expected, "{text}");
    }
}

/// Issue #49: a bare plural noun spelled like an essere verb's participle
/// can be a predicate: sono stati politici is "they have been political" and
/// "they are political states".
#[tokio::test]
async fn bare_plural_noun_after_essere_stays_a_predicate() {
    let a = run(Lang::It, "Sono stati politici.").await;
    let lists = group_lists(&a);
    assert!(
        lists.contains("essere indicative.present_perfect.3p [0, 1]"),
        "{lists:?}"
    );
    assert!(
        lists.contains("essere indicative.present.3p [0]"),
        "{lists:?}"
    );
    let stati = a.roles.iter().find(|r| r.token == "stati").unwrap();
    assert!(
        stati.roles.contains(&Some(Role::SubjectComplement)),
        "{:?}",
        stati.roles
    );
}

/// Issue #49: only essere has a compound tense; after the other linking
/// verbs an essere verb's participle is a state, also coordinated (sembra
/// nato e cresciuto qui), alone (sembra arrivato) and in a compound tense
/// (è rimasto scomparso).
#[tokio::test]
async fn participle_after_other_linking_verbs_stays_an_adjective() {
    for (text, word) in [
        ("Sembra nato e cresciuto qui.", "nato"),
        ("Sembra arrivato.", "arrivato"),
        ("È rimasto scomparso.", "scomparso"),
    ] {
        let a = run(Lang::It, text).await;
        let t = a.roles.iter().find(|r| r.token == word).unwrap();
        assert_eq!(t.roles, [Some(Role::SubjectComplement)], "{text}");
    }
}

/// Issue #49: morto also names a state (data/it/state-participles.txt), so
/// è morto is both "he died" and "he is dead"; the passive of a verb that
/// takes avere and its state reading stay.
#[tokio::test]
async fn state_participle_and_passive_after_essere_stay() {
    let a = run(Lang::It, "È morto.").await;
    let expected: BTreeSet<String> = [
        "morire indicative.present_perfect.3s [0, 1]",
        "essere indicative.present.3s [0]",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(group_lists(&a), expected);

    let a = run(Lang::It, "Tom è amato da tutti.").await;
    let lists = group_lists(&a);
    assert!(
        lists.contains("amare indicative.present.3s [1, 2]"),
        "{lists:?}"
    );
    assert!(
        lists.contains("essere indicative.present.3s [1]"),
        "{lists:?}"
    );
}

/// Issue #49: an adjective after essere agrees with it in number: sono
/// stanco is 1s only, sono stanchi 3p only. A noun phrase need not (siamo
/// una famiglia, sono io, è loro), and with an article a noun spelled like a
/// participle is that noun (è stato un successo).
#[tokio::test]
async fn subject_complement_agreement_after_essere() {
    let cases: [(&str, &[&str]); 4] = [
        ("Sono stanco.", &["essere indicative.present.1s [0]"]),
        ("Sono stanchi.", &["essere indicative.present.3p [0]"]),
        (
            "Siamo una famiglia.",
            &[
                "essere imperative.present.1p [0]",
                "essere indicative.present.1p [0]",
            ],
        ),
        ("È loro.", &["essere indicative.present.3s [0]"]),
    ];
    for (text, expected) in cases {
        let a = run(Lang::It, text).await;
        let expected: BTreeSet<String> = expected.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(group_lists(&a), expected, "{text}");
    }
    let a = run(Lang::It, "Sono io.").await;
    assert!(
        group_lists(&a).contains("essere indicative.present.3p [0]"),
        "{:?}",
        group_lists(&a)
    );
    // Only essere has the compound tense: after diventare the bare noun
    // stays a subject complement (diventano stati, "they become states").
    let a = run(Lang::It, "Diventano stati.").await;
    let stati = a.roles.iter().find(|r| r.token == "stati").unwrap();
    assert_eq!(stati.roles, [Some(Role::SubjectComplement)]);
    let a = run(Lang::It, "È stato un successo.").await;
    let successo = a.roles.iter().find(|r| r.token == "successo").unwrap();
    assert!(
        successo.roles.contains(&Some(Role::SubjectComplement)),
        "{:?}",
        successo.roles
    );
}

#[tokio::test]
async fn spanish_compound_tense() {
    let a = run(Lang::Es, "Tom me ha dado un bolígrafo.").await;
    every_parse(
        &a,
        &[Expected {
            tokens: vec![2, 3],
            auxiliaries: vec![2],
            participle: Some(3),
            verb: 3,
            lemma: "dar",
            cell: Some("indicative.present_perfect.3s"),
            voice: Voice::Active,
            progressive: false,
        }],
    );
}

#[tokio::test]
async fn spanish_perfect_progressive_is_named_after_its_auxiliaries() {
    let a = run(Lang::Es, "Ha estado lloviendo desde ayer.").await;
    let [g] = a.verb_groups[0].as_slice() else {
        panic!("{:?}", a.verb_groups[0])
    };
    check(
        g,
        &Expected {
            tokens: vec![0, 1, 2],
            auxiliaries: vec![0, 1],
            participle: None,
            verb: 2,
            lemma: "llover",
            cell: Some("indicative.present_perfect.3s"),
            voice: Voice::Active,
            progressive: true,
        },
    );
}

#[tokio::test]
async fn spanish_progressive_passive_keeps_its_progressive() {
    let a = run(Lang::Es, "El programa está siendo grabado.").await;
    let [g] = a.verb_groups[0].as_slice() else {
        panic!("{:?}", a.verb_groups[0])
    };
    check(
        g,
        &Expected {
            tokens: vec![2, 3, 4],
            auxiliaries: vec![2, 3],
            participle: Some(4),
            verb: 4,
            lemma: "grabar",
            cell: Some("indicative.present.3s"),
            voice: Voice::Passive,
            progressive: true,
        },
    );
}

#[tokio::test]
async fn french_compound_tense_with_a_clitic_before_the_auxiliary() {
    let a = run(Lang::Fr, "Je l'ai fait à sa demande.").await;
    let ai = a.tokens.iter().position(|t| t == "ai").unwrap();
    let fait = a.tokens.iter().position(|t| t == "fait").unwrap();
    every_parse(
        &a,
        &[Expected {
            tokens: vec![ai, fait],
            auxiliaries: vec![ai],
            participle: Some(fait),
            verb: fait,
            lemma: "faire",
            cell: Some("indicative.present_perfect.1s"),
            voice: Voice::Active,
            progressive: false,
        }],
    );
}

#[tokio::test]
async fn verb_groups_are_in_the_json() {
    let a = run(Lang::It, "Ho dato il libro a Maria.").await;
    let json = serde_json::to_value(&a).unwrap();
    assert_eq!(
        json["verb_groups"][0][0],
        serde_json::json!({
            "tokens": [0, 1],
            "auxiliaries": [0],
            "participle": 1,
            "verb": 1,
            "lemma": "dare",
            "cell": "indicative.present_perfect.1s",
            "voice": "active",
            "progressive": false,
        })
    );
}

#[tokio::test]
async fn spanish_copular_progressive_survives_negation() {
    // serv.negation passes está siendo up without aux_progressive; the
    // copula must still be one progressive group in every parse.
    let a = run(Lang::Es, "No está siendo difícil.").await;
    for groups in &a.verb_groups {
        let [g] = groups.as_slice() else {
            panic!("{groups:?}")
        };
        assert_eq!(
            (
                g.tokens.as_slice(),
                g.auxiliaries.as_slice(),
                g.verb,
                g.lemma.as_str()
            ),
            ([1, 2].as_slice(), [1].as_slice(), 2, "ser")
        );
        assert!(g.progressive && g.participle.is_none(), "{g:?}");
    }
}

#[tokio::test]
async fn spanish_stacked_progressives_are_one_group() {
    // sigue + (estando + lloviendo): v.progressive inside v.progressive.
    let fixture = FixtureLexicon::load(Path::new("tests/fixtures/es-handwritten.json")).unwrap();
    let lexicon = PatchedLexicon::new(Box::new(fixture), wordholder::patches().unwrap());
    let language = lang::load(Lang::Es).unwrap();
    let a = analyze(&lexicon, language.as_ref(), "Sigue estando lloviendo.")
        .await
        .unwrap();
    let stacked: Vec<&VerbGroup> = a
        .verb_groups
        .iter()
        .flatten()
        .filter(|g| g.tokens == [0, 1, 2])
        .collect();
    assert!(!stacked.is_empty(), "{:?}", a.verb_groups);
    for g in stacked {
        assert_eq!(
            (
                g.auxiliaries.as_slice(),
                g.verb,
                g.lemma.as_str(),
                g.progressive
            ),
            ([0, 1].as_slice(), 2, "llover", true)
        );
        assert!(
            ["indicative.present.3s", "imperative.present.2s"]
                .contains(&g.cell.as_deref().unwrap()),
            "{g:?}"
        );
    }
    for (tree, groups) in a.parses.iter().zip(&a.verb_groups) {
        assert_one_group_per_verb(groups, tree, "Sigue estando lloviendo.");
    }
}

/// The tokens a parse uses as a verb: `Verb` words, except a participle the
/// parse only uses like an adjective (`ap.participle`).
fn verb_tokens(tree: &Tree, parent: &str, out: &mut BTreeSet<usize>) {
    match tree {
        Tree::Node { rule, children, .. } => {
            for c in children {
                verb_tokens(c, rule, out);
            }
        }
        Tree::Leaf { token, morph, .. } => {
            if morph.cat == "Verb" && verb_use(morph, &[parent.to_owned()]).as_verb {
                out.insert(*token);
            }
        }
    }
}

/// The verb words each auxiliary rule joins directly: per node whose rule
/// passes an auxiliary's features up or marks a voice (`aux_tense`,
/// `voice`), its `Verb` children.
fn joined(tree: &Tree, out: &mut Vec<Vec<usize>>) {
    if let Tree::Node {
        feats, children, ..
    } = tree
    {
        if feats.keys().any(|k| k.starts_with("aux_") || k == "voice") {
            out.push(
                children
                    .iter()
                    .filter_map(|c| match c {
                        Tree::Leaf { token, morph, .. } if morph.cat == "Verb" => Some(*token),
                        _ => None,
                    })
                    .collect(),
            );
        }
        for c in children {
            joined(c, out);
        }
    }
}

/// Every word the parse uses as a verb is in exactly one group, every
/// group's word is one, and the words an auxiliary rule joins are in one
/// group, however the rules nest or wrap (está siendo under `serv.negation`,
/// sigue + estando lloviendo). The one exception: participles coordinated
/// after one auxiliary (sono nato e cresciuto) are groups that share it.
fn assert_one_group_per_verb(groups: &[VerbGroup], tree: &Tree, text: &str) {
    let mut verbs = BTreeSet::new();
    verb_tokens(tree, "", &mut verbs);
    let mut holders: BTreeMap<usize, Vec<&VerbGroup>> = BTreeMap::new();
    for g in groups {
        let mut parts = g.auxiliaries.clone();
        parts.push(g.verb);
        parts.sort();
        assert_eq!(g.tokens, parts, "{text}: {g:?}");
        assert!(g.participle.is_none_or(|p| p == g.verb), "{text}: {g:?}");
        for &t in &g.tokens {
            holders.entry(t).or_default().push(g);
        }
    }
    let grouped: BTreeSet<usize> = holders.keys().copied().collect();
    assert_eq!(grouped, verbs, "{text}: {groups:?}");
    let mut joins = Vec::new();
    joined(tree, &mut joins);
    let main_of = |t: usize| groups.iter().find(|g| g.verb == t);
    for words in joins {
        for (i, &a) in words.iter().enumerate() {
            for &b in &words[i + 1..] {
                let together = groups
                    .iter()
                    .any(|g| g.tokens.contains(&a) && g.tokens.contains(&b));
                let coordinated = match (main_of(a), main_of(b)) {
                    (Some(x), Some(y)) => {
                        !x.auxiliaries.is_empty() && x.auxiliaries == y.auxiliaries
                    }
                    _ => false,
                };
                assert!(
                    together || coordinated,
                    "{text}: {a} and {b} are joined but apart: {groups:?}"
                );
            }
        }
    }
    for (t, gs) in holders {
        if gs.len() > 1 {
            assert!(
                gs.iter()
                    .all(|g| g.auxiliaries.contains(&t) && g.auxiliaries == gs[0].auxiliaries),
                "{text}: token {t} is in {gs:?}"
            );
        }
    }
}

/// The rule above over the homograph sets: every kept parse of every
/// `STRIDE`th sentence (all 15,000 take three minutes unoptimized; with
/// `STRIDE = 1` and `--release`, half a minute, and they pass).
#[tokio::test]
async fn every_verb_is_in_one_group_over_the_homograph_sets() {
    for lang in [Lang::It, Lang::Es, Lang::Fr] {
        let fixture = FixtureLexicon::load(Path::new(&format!(
            "tests/fixtures/{}-set.json",
            lang.code()
        )))
        .unwrap();
        let lexicon = PatchedLexicon::new(Box::new(fixture), wordholder::patches().unwrap());
        let language = lang::load(lang).unwrap();
        let set =
            std::fs::read_to_string(format!("data/{}/homograph-set.jsonl", lang.code())).unwrap();
        let mut parses = 0;
        const STRIDE: usize = 4;
        for line in set.lines().step_by(STRIDE) {
            let item: serde_json::Value = serde_json::from_str(line).unwrap();
            let text = item["text"].as_str().unwrap();
            let a = analyze(&lexicon, language.as_ref(), text).await.unwrap();
            assert_eq!(a.verb_groups.len(), a.parses.len(), "{text}");
            for (tree, groups) in a.parses.iter().zip(&a.verb_groups) {
                assert_one_group_per_verb(groups, tree, text);
                parses += 1;
            }
        }
        assert!(parses > 2_000, "{}: only {parses} parses", lang.code());
    }
}

/// Issue #62: Word Holder's verb pages keep a sentence only when some parse
/// has a verb group for the page's verb. These sentences from the bere,
/// parlare and rispondere pages had no parse; one short sentence per rule
/// that fixed them follows. it-verb-pages.json is the API's answers for
/// their words.
#[tokio::test]
async fn verb_page_sentences_have_a_group_for_their_verb() {
    let fixture = FixtureLexicon::load(Path::new("tests/fixtures/it-verb-pages.json")).unwrap();
    let lexicon = PatchedLexicon::new(Box::new(fixture), wordholder::patches().unwrap());
    let language = lang::load(Lang::It).unwrap();
    for (text, verb, lemma, cell) in [
        // A comma before e or ma between two clauses.
        (
            "Per un po' sono stata veramente dipendente dalla Coca-Cola, e la bevevo ogni giorno.",
            12,
            "bere",
            "indicative.imperfect.1s",
        ),
        (
            "Mio padre beveva birra, e ora beve vino.",
            7,
            "bere",
            "indicative.present.3s",
        ),
        (
            "Puoi urlare, ma non risponderò.",
            5,
            "rispondere",
            "indicative.future.1s",
        ),
        (
            "Tom continuava a provare a chiamare Mary, però lei non rispondeva mai al telefono.",
            11,
            "rispondere",
            "indicative.imperfect.3s",
        ),
        // A plural adjective in -i agrees with a feminine noun (sensoriali,
        // strumentali); two infinitives share the verb.
        (
            "Per giovedì sto preparando prove sensoriali che possano divertire e insegnare qualcosa sui vini che berremo.",
            15,
            "bere",
            "indicative.future.1p",
        ),
        (
            "Rispondiamo alle polemiche strumentali.",
            0,
            "rispondere",
            "indicative.present.1p",
        ),
        // tanto quanto.
        (
            "Probabilmente non berrò tanto quanto Tom.",
            2,
            "bere",
            "indicative.future.1s",
        ),
        // A name first, before e and another name.
        (
            "Sami e Layla bevevano insieme.",
            3,
            "bere",
            "indicative.imperfect.3p",
        ),
        // The article + altro.
        (
            "Quando parli con gli altri, lo fai con le braccia incrociate.",
            1,
            "parlare",
            "indicative.present.2s",
        ),
        // An adverb between the subject and the verb.
        (
            "Io e Tom di solito ci parliamo in francese.",
            6,
            "parlare",
            "indicative.present.1p",
        ),
        // Two names in a row.
        (
            "Quante lingue si parlano in Nuova Guinea?",
            3,
            "parlare",
            "indicative.present.3p",
        ),
        // ecco + a noun phrase.
        (
            "Ecco la città di cui ti parlavo.",
            6,
            "parlare",
            "indicative.imperfect.1s",
        ),
        // l'altro giorno.
        (
            "Questo è il libro di cui parlavi l'altro giorno?",
            6,
            "parlare",
            "indicative.imperfect.2s",
        ),
        // A preposition before sì or no.
        (
            "Non parlo la sua lingua, risponda alle domande soltanto con \"sì\" o \"no\".",
            6,
            "rispondere",
            "imperative.present.3s",
        ),
        (
            "Risponda con sì o no.",
            0,
            "rispondere",
            "imperative.present.3s",
        ),
        // sentirsi + a participle that describes the subject.
        (
            "Mi sento chiamato in causa da questo post, quindi rispondo subito.",
            10,
            "rispondere",
            "indicative.present.1s",
        ),
        (
            "Mi sento chiamato in causa.",
            1,
            "sentire",
            "indicative.present.1s",
        ),
        // non solo … ma.
        (
            "In tal modo, non solo rispondiamo alle esigenze dei clienti ma le anticipiamo.",
            6,
            "rispondere",
            "indicative.present.1p",
        ),
        // Two phrases with the same preposition.
        (
            "Non rispondeva ai messaggi e alle chiamate.",
            1,
            "rispondere",
            "indicative.imperfect.3s",
        ),
    ] {
        let a = analyze(&lexicon, language.as_ref(), text).await.unwrap();
        let cells: BTreeSet<&str> = a
            .verb_groups
            .iter()
            .flatten()
            .filter(|g| g.verb == verb && g.lemma == lemma)
            .filter_map(|g| g.cell.as_deref())
            .collect();
        assert!(
            cells.contains(cell),
            "{text}: {lemma} {cell} not in {cells:?}"
        );
    }
}
