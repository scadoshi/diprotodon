trait Crlf {
    fn is_crlf(&self) -> bool;
    fn split_crlf(&self) -> Option<(&[u8], &[u8])>;
}

impl Crlf for [u8] {
    fn is_crlf(&self) -> bool {
        if let (Some(first_byte), Some(second_byte)) = (self.first(), self.get(1)) {
            *first_byte == b'\r' && *second_byte == b'\n'
        } else {
            false
        }
    }
    fn split_crlf(&self) -> Option<(&[u8], &[u8])> {
        let p = self
            .windows(2)
            .position(|w| w[0] == b'\r' && w[1] == b'\n')?;
        Some((&self[..p], &self[p + 2..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn abc() -> Vec<u8> {
        vec![b'a', b'b', b'c']
    }
    fn crlf() -> Vec<u8> {
        vec![b'\r', b'\n']
    }
    #[test]
    fn is_clrf_true() {
        assert!(crlf().is_crlf());
    }
    #[test]
    fn is_clrf_false() {
        assert!(!abc().is_crlf());
    }
    #[test]
    fn split_crlf_some() {
        let abc_crlf_abc: Vec<u8> = abc().into_iter().chain(crlf()).chain(abc()).collect();
        assert_eq!(
            abc_crlf_abc.split_crlf(),
            Some((abc().as_slice(), abc().as_slice()))
        );
        let abc_crlf: Vec<u8> = abc().into_iter().chain(crlf()).collect();
        assert_eq!(
            abc_crlf.split_crlf(),
            Some((abc().as_slice(), [].as_slice()))
        );
        let crlf_abc: Vec<u8> = crlf().into_iter().chain(abc()).collect();
        assert_eq!(
            crlf_abc.split_crlf(),
            Some(([].as_slice(), abc().as_slice()))
        );
    }
    #[test]
    fn split_crlf_none() {
        assert!([b'a'].split_crlf().is_none());
        assert!([].split_crlf().is_none());
    }
}
