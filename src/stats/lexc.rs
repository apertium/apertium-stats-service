extern crate hyper;

use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::io::{BufRead, BufReader};
use std::iter::FromIterator;

use super::StatsError;

use models::StatKind;

type LexiconEntry = (Vec<String>, HashSet<(String, BTreeSet<String>)>);
type Lexicons = HashMap<String, LexiconEntry>;

fn get_all_lexicons(lexicons: &Lexicons, root_lexicon: &str) -> BTreeSet<String> {
    let mut frontier = BTreeSet::from_iter(lexicons.get(root_lexicon).unwrap().clone().0);
    let frontier_clone = frontier.clone();
    frontier.extend(
        frontier_clone
            .iter()
            .flat_map(|lexicon| get_all_lexicons(lexicons, lexicon)),
    );
    frontier
}

pub fn get_stats(body: hyper::Chunk) -> Result<Vec<(StatKind, String)>, StatsError> {
    let mut current_lexicon: Option<String> = None;
    let mut lexicons: Lexicons = HashMap::new();

    lazy_static! {
        static ref CLEAN_RE: Regex = Regex::new(r"%(.)").unwrap(); // TODO: better name
        static ref CLEAN_COMMENTS_RE: Regex = Regex::new(r"!.*$").unwrap();
        // static ref
    }

    for line in BufReader::new(&*body).lines().filter_map(|line| line.ok()) {
        let clean_line_intermediate = CLEAN_RE.replace(&line, r"\1");
        let clean_line = CLEAN_COMMENTS_RE.replace(&clean_line_intermediate, "");
        if clean_line.starts_with("LEXICON") {
            let lexicon_name = clean_line.split_whitespace().nth(1).ok_or_else(|| {
                StatsError::Lexc(format!("LEXICON start missing <space> (L{})", 1))
            })?; // TODO: real line
            current_lexicon = Some(lexicon_name.to_string());
        } else if !clean_line.is_empty() && current_lexicon.is_some() {
            let line_error = format!("Unable to parse L{}", 1); // TODO: real line
            let token_count = clean_line.split_whitespace().count();

            if token_count >= 2 {
                if clean_line.contains(':') {
                    // TODO: me
                } else {
                    let mut split = clean_line
                        .split(';')
                        .next()
                        .ok_or_else(|| StatsError::Lexc(line_error.clone()))?
                        .trim()
                        .split_whitespace();
                    let lemma = split
                        .next()
                        .ok_or_else(|| StatsError::Lexc(line_error.clone()))?;
                    let continuation_lexicon = split
                        .next()
                        .ok_or_else(|| StatsError::Lexc(line_error.clone()))?
                        .trim()
                        .split('-')
                        .map(|x| x.to_string())
                        .collect::<BTreeSet<_>>();
                    // let gloss = if clean_line.contains('!') {
                    //     Some(clean_line.split('!').nth(1))
                    // } else {
                    //     None
                    // };

                    match lexicons.entry(current_lexicon.as_ref().unwrap().to_string()) {
                        Entry::Occupied(mut occupied) => {
                            occupied
                                .get_mut()
                                .1
                                .insert((lemma.to_string(), continuation_lexicon));
                        }
                        Entry::Vacant(vacant) => {
                            vacant.insert((
                                vec![],
                                HashSet::from_iter(vec![(lemma.to_string(), continuation_lexicon)]),
                            ));
                        }
                    };
                }
            } else if token_count == 1 {
                let lexicon_pointer = clean_line
                    .split(';')
                    .next()
                    .ok_or_else(|| StatsError::Lexc(line_error.clone()))?
                    .trim();
                if lexicon_pointer.contains(' ') {
                    // TODO: log error
                } else {
                    match lexicons.entry(current_lexicon.as_ref().unwrap().to_string()) {
                        Entry::Occupied(mut occupied) => {
                            occupied.get_mut().0.push(lexicon_pointer.to_string());
                        }
                        Entry::Vacant(vacant) => {
                            vacant.insert((vec![lexicon_pointer.to_string()], HashSet::new()));
                        }
                    };
                }
            } else {
                // TODO: log parse failure
            }
        }
    }

    if lexicons.contains_key("Root") {
        let reachable_lexicons = get_all_lexicons(&lexicons, "Root");
        let entries = reachable_lexicons
            .iter()
            .flat_map(|lexicon| lexicons.get(lexicon).unwrap().clone().1)
            .collect::<HashSet<_>>();
        Ok(vec![(StatKind::Stems, entries.len().to_string())])
    } else {
        Err(StatsError::Lexc(String::from("Missing Root lexicon")))
    }
}
