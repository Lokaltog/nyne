use std::collections::HashMap;
use std::io;

const MAX_SIZE: usize = 1024;

pub fn process(input: &str) -> String {
    input.to_uppercase()
}

fn helper() -> bool {
    true
}

pub struct Config {
    name: String,
    value: u32,
}

pub enum Status {
    Active,
    Inactive,
}

pub trait Processor {
    fn run(&self, input: &str) -> String;
    fn reset(&mut self);
}

impl Processor for Config {
    fn run(&self, input: &str) -> String {
        format!("{}: {}", self.name, input)
    }

    fn reset(&mut self) {
        self.value = 0;
    }
}

impl Config {
    pub fn new(name: String) -> Self {
        Self { name, value: 0 }
    }
}
