// Copyright (c) 2015 Takeru Ohta <phjgt308@gmail.com>
//
// This software is released under the MIT License,
// see the LICENSE file at the top-level directory.

use std::path::Path;
use std::fs::File;
use std::io::Result as IoResult;
use std::io::Write;
use std::io::BufWriter;
use std::io::Read;
use std::io::BufReader;
use byteorder::ByteOrder;
use byteorder::NativeEndian;
use WordId;
use Word;
use Char;
use common::CommonPrefixIter;
use common::NodeTraverse;

pub struct Trie {
    nodes: Vec<u64>,
    exts: Vec<u32>,
}

impl Trie {
    pub fn new(nodes: Vec<u64>, exts: Vec<u32>) -> Self {
        Trie {
            nodes: nodes,
            exts: exts,
        }
    }

    pub fn len(&self) -> usize {
        let mut count = 0;
        let mut node = NodeTraverser::new(self);
        loop {
            count += node.is_terminal() as usize + node.id_offset() as usize;
            for i in 0x00.. {
                if i == 0xFF {
                    return count;
                }

                let ch = (0xFF - i) as Char;
                if node.jump_char(ch).is_some() {
                    break;
                }
            }
        }
    }

    pub fn contains(&self, word: Word) -> bool {
        self.get_id(word).is_some()
    }

    pub fn get_id(&self, word: Word) -> Option<WordId> {
        let word_len = word.len();
        self.search_common_prefix(word).find(|m| word_len == m.1).map(|m| m.0)
    }

    pub fn search_common_prefix<'a, 'b>(&'a self,
                                        word: Word<'b>)
                                        -> CommonPrefixIter<'b, NodeTraverser<'a>> {
        CommonPrefixIter::new(word, NodeTraverser::new(self))
    }

    pub fn load<P: AsRef<Path>>(index_file_path: P) -> IoResult<Self> {
        let mut r = BufReader::new(try!(File::open(index_file_path)));
        let node_count = try!(read_u32(&mut r)) / 8;
        let ext_count = try!(read_u32(&mut r)) / 4;

        let mut nodes = Vec::with_capacity(node_count as usize);
        for _ in 0..node_count {
            nodes.push(try!(read_u64(&mut r)));
        }

        let mut exts = Vec::with_capacity(ext_count as usize);
        for _ in 0..ext_count {
            exts.push(try!(read_u32(&mut r)));
        }

        Ok(Trie::new(nodes, exts))
    }

    pub fn save<P: AsRef<Path>>(&self, index_file_path: P) -> IoResult<()> {
        let mut w = BufWriter::new(try!(File::create(index_file_path)));
        try!(write_u32(&mut w, self.nodes.len() as u32 * 8));
        try!(write_u32(&mut w, self.exts.len() as u32 * 4));
        for n in self.nodes.iter() {
            try!(write_u64(&mut w, *n));
        }
        for e in self.exts.iter() {
            try!(write_u32(&mut w, *e));
        }
        Ok(())
    }
}

fn read_u32<R: Read>(r: &mut R) -> IoResult<u32> {
    let mut buf = [0; 4];
    let size = try!(r.read(&mut buf));
    assert_eq!(size, buf.len());
    Ok(NativeEndian::read_u32(&buf))
}

fn read_u64<R: Read>(r: &mut R) -> IoResult<u64> {
    let mut buf = [0; 8];
    let size = try!(r.read(&mut buf));
    assert_eq!(size, buf.len());
    Ok(NativeEndian::read_u64(&buf))
}

fn write_u32<W: Write>(w: &mut W, n: u32) -> IoResult<()> {
    let mut buf = [0; 4];
    NativeEndian::write_u32(&mut buf, n);
    w.write_all(&mut buf)
}

fn write_u64<W: Write>(w: &mut W, n: u64) -> IoResult<()> {
    let mut buf = [0; 8];
    NativeEndian::write_u64(&mut buf, n);
    w.write_all(&mut buf)
}

fn base(n: u64) -> u32 {
    mask(n, 0, 29) as u32
}

fn is_terminal(n: u64) -> bool {
    mask(n, 31, 1) == 1
}

fn mask(n: u64, offset: usize, size: usize) -> u64 {
    (n >> offset) & ((1 << size) - 1)
}

pub struct NodeTraverser<'a> {
    node: u64,
    nodes: &'a Vec<u64>,
    exts: &'a Vec<u32>,
}

impl<'a> NodeTraverse for NodeTraverser<'a> {
    fn is_terminal(&self) -> bool {
        is_terminal(self.node)
    }

    fn id_offset(&self) -> u32 {
        let n = self.node;
        let node_type = mask(n, 29, 2);
        match node_type {
            0 => mask(n, 56, 8) as u32,
            1 => mask(n, 48, 16) as u32,
            2 => mask(n, 40, 24) as u32,
            3 => self.exts[mask(n, 40, 24) as usize],
            _ => unreachable!(),
        }
    }

    fn jump(&mut self, word: &mut Word) -> Option<()> {
        self.check_encoded_children(word)
            .and_then(|_| word.next().and_then(|ch| self.jump_char(ch)))
    }
}

impl<'a> NodeTraverser<'a> {
    pub fn new(trie: &'a Trie) -> Self {
        NodeTraverser {
            node: trie.nodes[0],
            nodes: &trie.nodes,
            exts: &trie.exts,
        }
    }

    fn jump_char(&mut self, ch: Char) -> Option<()> {
        let base = base(self.node) as usize;
        if self.nodes.len() <= base + ch as usize {
            return None;
        }

        let next = self.nodes[(base + ch as usize)];
        let chck = mask(next, 32, 8) as Char;
        if ch == chck {
            self.node = next;
            Some(())
        } else {
            None
        }
    }

    fn check_encoded_children(&mut self, word: &mut Word) -> Option<()> {
        let node_type = mask(self.node, 29, 2);
        let max = match node_type {
            0 => 2,
            1 => 1,
            _ => 0,
        };
        for i in 0..max {
            let c = mask(self.node, 40 + 8 * i, 8) as Char;
            if c == 0 {
                return Some(());
            }
            if !word.next().map_or(false, |ch| ch == c) {
                return None;
            }
        }
        Some(())
    }
}
