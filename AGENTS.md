# Instructions for AI agents

The rules for AI assistants and agents working in this repository are in
[CLAUDE.md](CLAUDE.md). They apply to every agent, whatever its vendor.
Read that file in full before making any change.

## Revue de code

Aucun code n'arrive dans `main` sans le verdict OK de l'agent reviewer (`.claude/agents/reviewer.md`) sur le diff complet
(`origin/main...HEAD`), quelle que soit la taille du changement, y compris quand la fusion directe est autorisée.
KO : corriger et relancer la revue. Les agents communs de `.claude/agents/` viennent de gmlabs-ch/agent-kit
(`scripts/sync-repo.sh`) : les modifier dans le kit, pas ici. Seuls les changements sans code (documentation,
contenu éditorial, données) échappent à la revue.
