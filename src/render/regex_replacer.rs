use regex::Regex;
use std::borrow::Cow;

pub type Replacement<'a> = (Regex, &'a str);

pub fn build_replacements<'a>(pairs: &[(&str, &'a str)]) -> Vec<(Regex, &'a str)> {
    pairs
        .iter()
        .map(|(pattern, replacement)| (Regex::new(pattern).expect("pattern"), *replacement))
        .collect()
}

pub fn replace<'a>(name: &'a str, replacements: &[Replacement]) -> Cow<'a, str> {
    let mut name: Cow<'_, str> = Cow::Borrowed(name);

    for (regex, replacement) in replacements {
        if let Cow::Owned(updated) = regex.replace(name.as_ref(), *replacement) {
            name = Cow::Owned(updated);
        }
    }

    name
}
