use rand::Rng;

const DRAFT_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const INTERNAL_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn random_id(alphabet: &[u8], len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| alphabet[rng.random_range(0..alphabet.len())] as char)
        .collect()
}

/// Public draft id: 12 chars of [a-z0-9] — short, unambiguous, URL-safe.
pub fn new_draft_id() -> String {
    random_id(DRAFT_ALPHABET, 12)
}

/// Internal id (versions): 20 chars of [a-zA-Z0-9].
pub fn new_internal_id() -> String {
    random_id(INTERNAL_ALPHABET, 20)
}
