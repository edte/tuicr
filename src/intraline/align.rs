// This alignment implementation is adapted from delta's within-line diff.
//
// Copyright 2020 Dan Davison
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::cmp::max;
use std::collections::VecDeque;

const DELETION_COST: usize = 2;
const INSERTION_COST: usize = 2;
const INITIAL_MISMATCH_PENALTY: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    NoOp,
    Deletion,
    Insertion,
}

use Operation::*;

#[derive(Clone, Debug)]
struct Cell {
    parent: usize,
    operation: Operation,
    cost: usize,
}

#[derive(Debug)]
pub(super) struct Alignment<'a> {
    pub x: Vec<&'a str>,
    pub y: Vec<&'a str>,
    table: Vec<Cell>,
    dim: [usize; 2],
}

impl<'a> Alignment<'a> {
    pub(super) fn new(x: Vec<&'a str>, y: Vec<&'a str>) -> Self {
        let dim = [y.len() + 1, x.len() + 1];
        let table = vec![
            Cell {
                parent: 0,
                operation: NoOp,
                cost: 0,
            };
            dim[0] * dim[1]
        ];
        let mut alignment = Self { x, y, table, dim };
        alignment.fill();
        alignment
    }

    fn fill(&mut self) {
        for i in 1..self.dim[1] {
            self.table[i] = Cell {
                parent: 0,
                operation: Deletion,
                cost: i * DELETION_COST + INITIAL_MISMATCH_PENALTY,
            };
        }
        for j in 1..self.dim[0] {
            self.table[j * self.dim[1]] = Cell {
                parent: 0,
                operation: Insertion,
                cost: j * INSERTION_COST + INITIAL_MISMATCH_PENALTY,
            };
        }

        for (i, x_i) in self.x.iter().enumerate() {
            for (j, y_j) in self.y.iter().enumerate() {
                let (left, diag, up) =
                    (self.index(i, j + 1), self.index(i, j), self.index(i + 1, j));
                let candidates = [
                    Cell {
                        parent: up,
                        operation: Insertion,
                        cost: self.mismatch_cost(up, INSERTION_COST),
                    },
                    Cell {
                        parent: left,
                        operation: Deletion,
                        cost: self.mismatch_cost(left, DELETION_COST),
                    },
                    Cell {
                        parent: diag,
                        operation: NoOp,
                        cost: if x_i == y_j {
                            self.table[diag].cost
                        } else {
                            usize::MAX
                        },
                    },
                ];
                let index = self.index(i + 1, j + 1);
                self.table[index] = candidates
                    .iter()
                    .min_by_key(|cell| cell.cost)
                    .expect("alignment candidate list is non-empty")
                    .clone();
            }
        }
    }

    fn mismatch_cost(&self, parent: usize, basic_cost: usize) -> usize {
        self.table[parent].cost
            + basic_cost
            + usize::from(self.table[parent].operation == NoOp) * INITIAL_MISMATCH_PENALTY
    }

    fn operations(&self) -> Vec<Operation> {
        let mut operations = VecDeque::with_capacity(max(self.x.len(), self.y.len()));
        let mut cell = &self.table[self.index(self.x.len(), self.y.len())];
        loop {
            operations.push_front(cell.operation);
            if cell.parent == 0 {
                break;
            }
            cell = &self.table[cell.parent];
        }
        Vec::from(operations)
    }

    pub(super) fn coalesced_operations(&self) -> Vec<(Operation, usize)> {
        run_length_encode(self.operations())
    }

    fn index(&self, i: usize, j: usize) -> usize {
        j * self.dim[1] + i
    }
}

fn run_length_encode<T: Copy + PartialEq>(sequence: Vec<T>) -> Vec<(T, usize)> {
    if sequence.is_empty() {
        return Vec::new();
    }

    let mut encoded = Vec::with_capacity(sequence.len());
    let end = sequence.len();
    let (mut i, mut j) = (0, 1);
    let mut current = sequence[i];
    loop {
        if j == end || sequence[j] != current {
            encoded.push((current, j - i));
            if j == end {
                return encoded;
            }
            current = sequence[j];
            i = j;
        }
        j += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_group_changed_tokens_like_delta() {
        let alignment = Alignment::new(vec!["", "A", "A", "B", "B"], vec!["", "A", "B"]);
        assert_eq!(
            alignment.operations(),
            vec![NoOp, NoOp, Deletion, Deletion, NoOp]
        );
    }

    #[test]
    fn should_prefer_moved_token_as_delete_then_insert() {
        let alignment = Alignment::new(vec!["", "a", "b"], vec!["", "b", "a"]);
        assert_eq!(
            alignment.operations(),
            vec![NoOp, Deletion, NoOp, Insertion]
        );
    }
}
