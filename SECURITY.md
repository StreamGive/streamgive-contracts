# Security Policy

## Reporting a vulnerability

If you believe you have found a security vulnerability in StreamGive, please report it privately and responsibly.

Do not open a public GitHub issue for security reports. Please email us at:

- security@streamgive.io

Include the following information in your report:

- A clear description of the vulnerability
- The affected contract, module, or component
- Steps to reproduce the issue or a proof of concept
- The potential impact and severity
- Any suggested remediation or mitigation

We ask that you give us a reasonable amount of time to investigate and remediate the issue before disclosing it publicly.

## Response expectations

We will do our best to acknowledge receipt of your report within 5 business days. We may ask for additional details to validate the issue and confirm the affected scope.

We appreciate responsible disclosure and will work with researchers to resolve confirmed vulnerabilities in a timely manner.

## Supported versions

The project currently supports the latest commit on the `main` branch and the latest published release. Security fixes are prioritized for the current supported branch and release train.

## Disclosure policy

Please do not disclose the vulnerability publicly until we have had a chance to assess and remediate it. If a fix is not available promptly, we may coordinate a reasonable disclosure timeline.

If you are reporting a vulnerability in a dependency or third-party library affecting this project, please include the dependency details and any reproduction steps.

## Scope

This policy covers vulnerabilities in the StreamGive smart contracts, deployment tooling, and related repository code maintained in this project.

We do not accept reports for general bugs that do not create a security impact, or for issues already known to the project and publicly documented.

## Known-bad commits: do not check out

Between 2026-10-01 and 2026-10-18 the `main` branch briefly contained malware (the "PolinRider" campaign), added through a forged merge commit that was credited to a maintainer. The files were removed in commit `9e8681b` ("chore: remove malicious files injected through PR #234") and the current `main` is clean.

The malicious files remain in the git history, and in the hidden pull request refs that GitHub keeps. They were:

- `.vscode/tasks.json` and `.vscode/settings.json`, which make VS Code run a task automatically when the folder is opened
- `public/fonts/fa-*` files that are named like fonts but are really JavaScript
- the same payload hidden in config files such as `postcss.config.mjs`

What this means for you:

- A normal `git clone` is safe. It checks out the current `main`, which is clean.
- Do not check out, build, or open in an editor any commit made before `9e8681b`, in particular `a627adc` (2026-10-01) through `5f14b3d` (2026-10-18). Do not check out the branch of any pull request opened before 2026-10-19 either.
- To check that a commit is safe: `git merge-base --is-ancestor 9e8681b <commit> && echo safe`. If it prints nothing, treat that commit as unsafe.
- Branches and forks created before 2026-10-19 may still carry these files. Rebase them onto the current `main` before you work on them.
- If you opened this repository in VS Code or ran a build on an older commit, treat that computer as possibly infected: scan it, then rotate your passwords, SSH keys and tokens.

If you find any of these files in a pull request, or are unsure about one, report it as described above instead of opening or running it.
