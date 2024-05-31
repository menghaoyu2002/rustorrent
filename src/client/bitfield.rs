use core::fmt;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug)]
pub struct Bitfield {
    bitfield: Vec<bool>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OutOfBoundsError {
    index: usize,
    len: usize,
}

impl Display for OutOfBoundsError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "Index out of bounds: index = {}, len = {}",
            self.index, self.len
        )
    }
}

impl Bitfield {
    pub fn new(size: usize) -> Self {
        let bitfield = vec![false; size];
        Self { bitfield }
    }

    pub fn len(&self) -> usize {
        self.bitfield.len()
    }

    pub fn set(&mut self, index: usize, value: bool) -> Result<(), OutOfBoundsError> {
        if index >= self.bitfield.len() {
            return Err(OutOfBoundsError {
                index,
                len: self.len(),
            });
        }
        self.bitfield[index] = value;
        Ok(())
    }

    pub fn is_set(&self, index: usize) -> Result<bool, OutOfBoundsError> {
        if index >= self.bitfield.len() {
            return Err(OutOfBoundsError {
                index,
                len: self.len(),
            });
        }

        Ok(self.bitfield[index])
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for chunk in self.bitfield.chunks(8) {
            let mut byte = 0;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << (7 - i);
                }
            }
            bytes.push(byte);
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitfield() {
        let mut bitfield = Bitfield::new(10);
        assert_eq!(bitfield.len(), 10);
        assert_eq!(bitfield.is_set(0).unwrap(), false);
        assert_eq!(bitfield.is_set(1).unwrap(), false);
        assert_eq!(bitfield.is_set(2).unwrap(), false);
        assert_eq!(bitfield.is_set(3).unwrap(), false);
        assert_eq!(bitfield.is_set(4).unwrap(), false);
        assert_eq!(bitfield.is_set(5).unwrap(), false);
        assert_eq!(bitfield.is_set(6).unwrap(), false);
        assert_eq!(bitfield.is_set(7).unwrap(), false);
        assert_eq!(bitfield.is_set(8).unwrap(), false);
        assert_eq!(bitfield.is_set(9).unwrap(), false);

        bitfield.set(0, true).unwrap();
        bitfield.set(1, true).unwrap();
        bitfield.set(2, true).unwrap();
        bitfield.set(3, true).unwrap();
        bitfield.set(4, true).unwrap();
        bitfield.set(5, true).unwrap();
        bitfield.set(6, true).unwrap();
        bitfield.set(7, true).unwrap();
        bitfield.set(8, true).unwrap();
        bitfield.set(9, true).unwrap();

        assert_eq!(bitfield.is_set(0).unwrap(), true);
        assert_eq!(bitfield.is_set(1).unwrap(), true);
        assert_eq!(bitfield.is_set(2).unwrap(), true);
        assert_eq!(bitfield.is_set(3).unwrap(), true);
        assert_eq!(bitfield.is_set(4).unwrap(), true);
        assert_eq!(bitfield.is_set(5).unwrap(), true);
        assert_eq!(bitfield.is_set(6).unwrap(), true);
        assert_eq!(bitfield.is_set(7).unwrap(), true);
        assert_eq!(bitfield.is_set(8).unwrap(), true);
        assert_eq!(bitfield.is_set(9).unwrap(), true);

        let bytes = bitfield.to_bytes();
        assert_eq!(bytes, vec![0b11111111, 0b11000000]);

        bitfield.set(7, false).unwrap();
        bitfield.set(3, false).unwrap();

        assert_eq!(
            bitfield.set(23, false),
            Err(OutOfBoundsError { index: 23, len: 10 })
        );

        assert_eq!(
            bitfield.is_set(23),
            Err(OutOfBoundsError { index: 23, len: 10 })
        );

        let bytes = bitfield.to_bytes();
        assert_eq!(bytes, vec![0b11101110, 0b11000000]);
    }
}
