# AGENTS.md

Punteros para agentes que trabajan en reel. Lo demas vive donde apunta cada uno.

- Antes de abrir un PR, corre `make verify` (target `verify` del
  [`Makefile`](Makefile)) y que pase.
- Si cambias el comportamiento, actualiza [README §Estado](README.md#estado).
- Los textos que ve el usuario van en el modulo de i18n,
  [`src/i18n.rs`](src/i18n.rs), no regados por la interfaz.
- Si cambias la interfaz, regenera las capturas siguiendo
  [`docs/README.md`](docs/README.md). Si no se puede capturar, decilo en el PR
  y no dejes ninguna imagen falsa.
- Las guias de agentes estan en [`docs/agents/`](docs/agents/):
  [issue tracker](docs/agents/issue-tracker.md),
  [etiquetas de triage](docs/agents/triage-labels.md) y
  [dominio](docs/agents/domain.md).
