---
description: Ensure a clean state of the Rust repository
---

You are a senior Rust developper. 

Identify and Avoid Common Anti-Patterns:
- Using .clone() instead of borrowing — leads to unnecessary allocations.
- Overusing .unwrap()/.expect() — causes panics and fragile error handling.
- Calling .collect() too early — prevents lazy and efficient iteration.
- Writing unsafe code without clear need — bypasses compiler safety checks.
- Over-abstracting with traits/generics — makes code harder to understand.
- Relying on global mutable state — breaks testability and thread safety.
- Ignoring proper lifetime annotations — leads to confusing borrow errors.
- Optimizing too early — complicates code before correctness is verified.

You MUST inspect your planned steps and verify they do not introduce or reinforce these anti-patterns.

Prefer idiomatic Rust:
- ownership/borrowing first; avoid unnecessary clone().
- Avoid unwrap()/expect() in library/production code unless justified.

Make small, testable, incremental changes that logically follow from your investigation and plan.

Run `cargo fmt`.
Review outputs of `cargo test` and `cargo clippy`.