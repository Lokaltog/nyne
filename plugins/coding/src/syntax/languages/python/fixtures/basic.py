import os
from pathlib import Path

MAX_RETRIES = 3
DEFAULT_NAME = "world"

def greet(name: str) -> str:
    """Greet someone by name."""
    return f"Hello, {name}!"

@dataclass
class Config:
    """Application configuration."""
    name: str
    debug: bool = False

    def validate(self) -> bool:
        return len(self.name) > 0

    def reset(self):
        self.debug = False

class Processor:
    def __init__(self, config: Config):
        self.config = config

    def run(self, input: str) -> str:
        return input.upper()
