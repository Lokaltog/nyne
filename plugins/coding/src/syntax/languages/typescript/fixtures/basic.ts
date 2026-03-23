import { readFile } from "fs";
import type { Config } from "./types";

const MAX_RETRIES = 3;

export function greet(name: string): string {
    return `Hello, ${name}!`;
}

function helper(): boolean {
    return true;
}

export interface Processor {
    run(input: string): string;
    reset(): void;
}

export class AppConfig {
    name: string;

    constructor(name: string) {
        this.name = name;
    }

    validate(): boolean {
        return this.name.length > 0;
    }

    reset(): void {
        this.name = "";
    }
}

export enum Status {
    Active = "active",
    Inactive = "inactive",
}

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };
