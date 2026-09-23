# Security policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately to
`drfraudsworth@gmail.com`. Include the affected version or commit, impact,
reproduction steps and any suggested mitigation. Do not open a public issue
before a fix or disclosure plan is agreed.

The maintainers will acknowledge the report, investigate it and coordinate
responsible disclosure. This repository does not currently promise a bug
bounty.

## Supported versions

Security fixes are applied to the latest published crate and the current
`main` branch. Older releases may be asked to upgrade.

## Scope and audit status

This repository contains an off-chain Jupiter adapter. Historical on-chain
source and internal audit artifacts are available in the
[protocol repository](https://github.com/MetalLegBob/drfraudsworth). Its public
`main` branch is not a source-to-binary attestation for the September 2026
mainnet upgrades. The current reproducible-build and finalized-chain readback
records can be supplied during integration review.

The bundled IDLs are byte-identical to the reviewed mainnet release artifacts.
Neither the internal audit material nor the build/readback evidence is an
independent third-party security audit, and it is not represented as one.
