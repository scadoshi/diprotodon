# Discipline Rules

This project IS the prep. The point is rebuilding the muscle, not shipping a product. AI shortcuts that skip the muscle defeat the project.

## Write by hand

- RESP parser
- Command dispatch
- Core data structures
- The connection loop
- The first version of every test

## Lean on AI for

- `Cargo.toml` dependency setup
- Test scaffolding (after you've written one test by hand)
- README skeletons
- Explaining a concept you don't understand
- Debugging help *after* you've read the compiler error yourself

## When you hit a compiler error

Read it. Then read it again. Then try to fix it. Then ask AI. Compiler errors are the fastest Rust teacher and you don't get that lesson if AI fixes it for you.

## Commit guidelines

One feature, one commit. Easier to:
- Compare Rust vs Go sibling later
- Show the journey in interview conversation
- Roll back if a milestone goes sideways

Message conventions:
- Concise, one-line messages (multi-line only when many changes)
- Group related files logically
- No emojis
- Use `git diff` to understand changes before committing
- **Never** include AI-agent signatures in commits (no "Co-Authored-By: Claude...", no "Generated with...")

## Tests

- Write the test for a feature *after* the feature works manually, by hand, no AI
- After the first hand-written test, AI can scaffold sibling tests
- Integration tests over unit tests for the protocol layer — connect a real client, send real bytes
