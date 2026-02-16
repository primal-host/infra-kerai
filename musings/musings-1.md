# Project naming for AST-based version control (now Kerai / ker.ai)

*Shared by Billy — a conversation between Claude and Billy*

---

**Billy:** I'm looking for words that start or end in "ast" that might make a good project name for something like the discussion at https://gist.github.com/gritzko/6e81b5391eacb585ae207f5e634db07e and https://news.ycombinator.com/item?id=47022238

---

**Claude:** Great context. This is about a next-gen version control system that works with AST trees instead of blobs/files, uses CRDTs for deterministic merges, and treats code as a structured database rather than a filesystem. The "ast" connection is perfect since AST (Abstract Syntax Tree) is literally the core concept.

Here are some candidates, grouped by vibe:

**Structural / Foundational**
- **Mast** — the structural support that holds everything up. Short, strong, memorable. Also evokes navigation/sailing (charting a course through code).
- **Steadfast** — reliability, determinism, formal guarantees. Speaks to the CRDT promise of deterministic merges.

**Breaking with convention**
- **Iconoclast** — literally "breaker of icons." The whole thesis is rejecting git orthodoxy. Strong but maybe a mouthful for a CLI tool.
- **Blast** — breaking through, speed, power. `blast merge`, `blast diff` reads nicely.

**Diffing / Comparison**
- **Contrast** — diffing is a core operation, and this system promises *better* diffs. Contains "ast" right in the middle.

**Distribution / Collaboration**
- **Broadcast** — syncing, replication, CRDT propagation. Fitting for a system built on replicated data types.
- **Cast** — short, clean. "Casting" code into structure. Also plays on typecast, forecast, etc. `cast commit`, `cast merge` flows well.

**Intelligence / Insight**
- **Astute** — smart, perceptive. Fits the "code insight" and query language ambitions.

**Speed / Scale**
- **Fast** — obvious but maybe *too* generic. Though `fast merge` is appealing.
- **Vast** — scale, monorepo ambitions.

**Forward-looking**
- **Forecast** — predicting outcomes, looking ahead. Nice for a system that promises deterministic merges (you can *forecast* the result).

My top picks for a project name would be:
1. **Mast** — short, punchy, structural metaphor, contains AST, great as a CLI command
2. **Cast** — versatile, clean, implies transformation ("casting" code into trees)
3. **Contrast** — directly evokes the diffing use case, AST embedded naturally
4. **Iconoclast** — bold statement of intent, though better as a tagline than a daily-use command
5. **Forecast** — unique angle emphasizing determinism and predictability

Any of these resonate, or want me to explore a different direction?

---

**Billy:** Are you aware of any notable projects that use the suggestions above, so we might steer away from those?

---

**Claude:** Here's the collision report for each suggestion:

**Definitely steer clear:**
- **Blast** — NCBI BLAST is one of the most widely used bioinformatics tools in the world. Extremely well-known in tech/science circles.
- **Cast** — CAST Software (castsoftware.com) is a major player in software intelligence and code analysis — essentially the same domain as this project. That's a direct collision.
- **Contrast** — Contrast Security is a well-funded application security company, also in the dev tools space. Too close for comfort.
- **Vast** — VAST Data is a $9B+ AI infrastructure/storage company. Very high profile in the data infrastructure world.
- **Mast** — Heavily overloaded. There's the Mastodon iOS client, plus Edinburgh's MAST research group (Machine learning for Analysis of Source code Text — almost exactly this domain), plus several bioinformatics tools.

**Probably fine:**
- **Forecast** — Meta had a prediction market app called Forecast but shut it down. There are R/Python forecasting packages, but these are in a completely different domain. Low risk of confusion.
- **Steadfast** — No notable software projects I'm aware of. Clean.
- **Iconoclast** — No notable software projects. Clean, though it's a mouthful for a CLI tool.
- **Astute** — No major software conflicts. Clean.

So from the original list, **Steadfast**, **Forecast**, and **Astute** are the safest. Want me to brainstorm more "ast" words now that we've narrowed the field? There are some good ones I didn't explore yet — things like Ballast, Lambast, Dynast, Clast, or starting with ast: Aster, Astral, Asterisk.

---

**Billy:** I think I'll go with astute.

---

**Claude:** Nice choice. It's clean, short, easy to type, and the meaning fits well — a system that actually understands code structure rather than treating it as dumb blobs. Good luck with it, Billy.

---

*The project was later renamed to **Kerai** (ker.ai) — from 家来 (kerai), a Japanese word meaning retainer or servant. The name reflects the project's evolution toward AI agents that serve developers: loyal retainers working the codebase. The domain `ker.ai` was already held. The repo lives at `primal-host/kerai` on Gitea and GitHub.*
