# Contributing to Skypier Blackhole

Thank you for your interest in contributing to Skypier Blackhole! We welcome contributions from the community.

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/YOUR-USERNAME/skypier-blackhole.git
   cd skypier-blackhole
   ```
3. **Set up the development environment**:
   ```bash
   # Ensure you have Rust 1.70+ installed
   rustup update stable
   
   # Build the project
   cargo build
   
   # Run tests to verify setup
   cargo test
   ```

## Development Workflow

1. Create a feature branch from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make your changes, following our code style guidelines

3. Run the test suite:
   ```bash
   cargo test
   ```

4. Run the linter:
   ```bash
   cargo clippy -- -D warnings
   ```

5. Format your code:
   ```bash
   cargo fmt
   ```

6. Commit your changes with a descriptive message:
   ```bash
   git commit -m "Add feature: description of your change"
   ```

7. Push to your fork and open a Pull Request

## Code Style Guidelines

- Follow standard Rust conventions and idioms
- Use `cargo fmt` to format all code
- Ensure `cargo clippy` passes without warnings
- Write documentation comments for public APIs
- Keep functions focused and reasonably sized
- Prefer explicit error handling over `.unwrap()`

## Testing

- Add tests for new functionality
- Ensure all existing tests pass before submitting
- Integration tests go in the `tests/` directory
- Use the scripts in `scripts/` for manual testing:
  - `scripts/test-dns.sh` - DNS functionality
  - `scripts/test-signals.sh` - Signal handling
  - `scripts/test-wildcards.sh` - Wildcard matching
  - `scripts/test-cli.sh` - CLI commands

## Pull Request Guidelines

- Keep PRs focused on a single feature or fix
- Write a clear description of what the PR does
- Reference any related issues
- Ensure CI passes before requesting review
- Be responsive to feedback and review comments

## Reporting Issues

When reporting bugs, please include:

- Your operating system and version
- Rust version (`rustc --version`)
- Steps to reproduce the issue
- Expected vs actual behavior
- Relevant log output

## Feature Requests

We welcome feature suggestions! Please open an issue describing:

- The problem you're trying to solve
- Your proposed solution
- Any alternatives you've considered

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project (MIT/Apache-2.0 dual license).

## Questions?

Feel free to open a Discussion on GitHub if you have questions about contributing.
