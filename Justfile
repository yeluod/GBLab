default:
    @just --list

install:
    pnpm install --frozen-lockfile

dev:
    pnpm run tauri:dev

test:
    pnpm run test:run
    cargo test --workspace --locked

verify:
    pnpm run check
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --locked
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

build:
    pnpm run tauri:build
