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

fn make_parse_error(line_number: usize, error: &str) -> StatsError {
    StatsError::Lexc(format!("Unable to parse L{}: {}", line_number, error))
}

fn update_lexicons(
    current_lexicon: &str,
    lexicons: &mut Lexicons,
    lemma: &str,
    continuation_lexicon: BTreeSet<String>,
) {
    match lexicons.entry(current_lexicon.to_string()) {
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

fn parse_line(
    line: &str,
    line_number: usize,
    current_lexicon: &str,
    lexicons: &mut Lexicons,
) -> Result<(), StatsError> {
    lazy_static! {
        static ref SPLIT_RE: Regex = Regex::new(r"^(.+?):([^;]+);(?:\s+!\s+(.+))?").unwrap(); // TODO: better name
    }

    let token_count = line.split_whitespace().count();

    if token_count >= 3 {
        if line.contains(':') {
            let split = SPLIT_RE
                .captures_iter(line)
                .next()
                .ok_or_else(|| make_parse_error(line_number, "missing tokens"))?;

            let lemma = split
                .get(0)
                .ok_or_else(|| make_parse_error(line_number, "missing lemma"))?
                .as_str()
                .trim();
            let continuation_lexicon = split
                .get(1)
                .ok_or_else(|| make_parse_error(line_number, "missing continuation lexicon"))?
                .as_str()
                .split_whitespace()
                .last()
                .ok_or_else(|| make_parse_error(line_number, "missing continuation lexicon"))?
                .split('-')
                .map(|x| x.to_string())
                .collect::<BTreeSet<_>>();
            // let gloss = split.get(2).ok_or_else(|| make_parse_error(line_number, "missing gloss"))?;

            update_lexicons(current_lexicon, lexicons, lemma, continuation_lexicon);
            Ok(())
        } else {
            let mut split = line.split(';')
                .next()
                .ok_or_else(|| make_parse_error(line_number, "failed to split at ;"))?
                .trim()
                .split_whitespace();
            let lemma = split
                .next()
                .ok_or_else(|| make_parse_error(line_number, "failed to get lemma"))?;
            let continuation_lexicon = split
                .next()
                .ok_or_else(|| make_parse_error(line_number, "failed to get continuation lexicon"))?
                .trim()
                .split('-')
                .map(|x| x.to_string())
                .collect::<BTreeSet<_>>();
            // let gloss = if line.contains('!') {
            //     Some(line.split('!').nth(1))
            // } else {
            //     None
            // };

            update_lexicons(current_lexicon, lexicons, lemma, continuation_lexicon);
            Ok(())
        }
    } else if token_count == 2 {
        let lexicon_pointer = line.split(';')
            .next()
            .ok_or_else(|| make_parse_error(line_number, "failed to get lexicon pointer"))?
            .trim();
        if lexicon_pointer.contains(' ') {
            Err(make_parse_error(line_number, "lexicon pointer has space"))
        } else {
            match lexicons.entry(current_lexicon.to_string()) {
                Entry::Occupied(mut occupied) => {
                    occupied.get_mut().0.push(lexicon_pointer.to_string());
                }
                Entry::Vacant(vacant) => {
                    vacant.insert((vec![lexicon_pointer.to_string()], HashSet::new()));
                }
            };

            Ok(())
        }
    } else {
        Err(make_parse_error(line_number, "missing tokens"))
    }
}

pub fn get_stats(body: hyper::Chunk) -> Result<Vec<(StatKind, String)>, StatsError> {
    let mut current_lexicon: Option<String> = None;
    let mut lexicons: Lexicons = HashMap::new();

    lazy_static! {
        static ref ESCAPE_RE: Regex = Regex::new(r"%(.)").unwrap();
        static ref CLEAN_COMMENTS_RE: Regex = Regex::new(r"!.*$").unwrap();
    }

    for (line_number, line) in BufReader::new(&*body)
        .lines()
        .filter_map(|line| line.ok())
        .enumerate()
    {
        let unescaped_line = ESCAPE_RE.replace_all(&line, r"\1");
        let without_comments_line = CLEAN_COMMENTS_RE.replace(&unescaped_line, "");
        let clean_line = without_comments_line.trim();

        if clean_line.starts_with("LEXICON") {
            let lexicon_name = clean_line.split_whitespace().nth(1).ok_or_else(|| {
                StatsError::Lexc(format!("LEXICON start missing <space> (L{})", line_number))
            })?;
            current_lexicon = Some(lexicon_name.to_string());
        } else if !clean_line.is_empty() && current_lexicon.is_some() {
            if let Err(err) = parse_line(
                clean_line,
                line_number,
                current_lexicon.as_ref().unwrap(),
                &mut lexicons,
            ) {
                println!("{:?}", err); // TODO: log
            }
        }
    }

    if lexicons.contains_key("Root") {
        let reachable_lexicons = get_all_lexicons(&lexicons, "Root");
        let entries = reachable_lexicons
            .iter()
            .flat_map(|lexicon| lexicons[lexicon].clone().1)
            .collect::<HashSet<_>>();
        Ok(vec![(StatKind::Stems, entries.len().to_string())])
    } else {
        Err(StatsError::Lexc(String::from("Missing Root lexicon")))
    }
}
