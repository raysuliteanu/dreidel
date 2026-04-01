# [LOW] GitHub Actions not pinned to commit SHAs

## Location
`.github/workflows/ci.yml` — all `uses:` references

## Description
All third-party GitHub Actions are pinned to mutable version tags rather than immutable
commit SHAs:

```yaml
- uses: actions/checkout@v4
- uses: dtolnay/rust-toolchain@stable
- uses: Swatinem/rust-cache@v2
- uses: rustsec/audit-check@v2
- uses: EmbarkStudios/cargo-deny-action@v2
```

Version tags (e.g. `@v4`, `@v2`) are mutable references — the action author can move the
tag to a different commit at any time, including maliciously. A compromised tag could
execute arbitrary code in CI with access to repository secrets (`GITHUB_TOKEN`).

This is a supply chain security concern. The GitHub security hardening guide recommends
pinning all actions to full commit SHAs.

## Impact
- A compromised or malicious update to any action could exfiltrate `GITHUB_TOKEN` or
  inject malicious builds.
- Applies to all actions including `rustsec/audit-check` (a security-focused action that
  ironically runs in a privileged context).

## Recommended Fix
Pin each action to a specific commit SHA. Example:

```yaml
# actions/checkout v4.2.2
- uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683

# dtolnay/rust-toolchain (stable, 2025-03-01 snapshot)
- uses: dtolnay/rust-toolchain@888302dc8e06d7f7f31631f5e78feba87e9efcbf

# Swatinem/rust-cache v2.7.5
- uses: Swatinem/rust-cache@f0deed1e0edfc6a9be95417288c0e1099b1eeec3
```

Tools like [Dependabot](https://docs.github.com/en/code-security/dependabot/working-with-dependabot/keeping-your-actions-up-to-date-with-dependabot)
or [Renovate](https://docs.renovatebot.com/) can automate SHA updates with PR reviews.
