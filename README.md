<p align="center">
  <img src="https://raw.githubusercontent.com/fuderis/osy-kernel/main/assets/logo.png" alt="Logo" width="80" />
</p>

<h1 align="center">Osy Kernel</h1>
<p align="center">
  <strong>Deterministic, Token-Efficient Engine for Next-Gen AI Assistants</strong><br>
  <code>microservice-architecture</code> • <code>process-isolated</code> • <code>token-optimized</code> • <code>ultra-fast</code>
</p>

<img src="https://raw.githubusercontent.com/fuderis/osy-kernel/main/assets/cover.png" alt="Cover" width="100%" />

**Osy** is an open-source, high-performance orchestration kernel written in Rust. It brings Unix philosophy and microservice isolation to LLM agents — treating skills as lightweight, atomic CLI-like utilities operating over lightning-fast IPC.

Traditional agentic frameworks often suffer from uncontrolled agent autonomy, runaway token usage, and context pollution. Osy solves these issues at a systems level: orchestration happens in a pure chat loop, agent tasks run in isolated processes with zero access to the user context, and only clean final outputs are fed back to the orchestrator.

> ⚠️ **EXPERIMENTAL:** Osy is under active evolution with rapid refactoring of IPC contracts and internal pipelines.
> 
> **Storage & RAM Warning:** Embedded databases (LanceDB & Sled) run directly inside the kernel process and may consume significant RAM and I/O under heavy workloads. External DB driver abstractions are planned for future releases.

---

## Key Features

* **Microservice Agent Isolation:** Agents run as lightweight IPC servers. Every skill execution runs in its own isolated context without cluttering the main conversation history.
* **Extreme Token Scrubbing:** Technical payloads, raw JSONs, and tool calls never leak into the main dialogue. You pay only for meaningful orchestrator interactions.
* **Deterministic Star Topology:** Agents are strict executors managed directly by the Kernel. They cannot spam each other or trigger infinite recursive loops.
* **Zero-Overhead IPC & SSE Transport:** Communication moves strictly over Unix Domain Sockets (or Named Pipes for Windows) via native streaming protocols, avoiding HTTP network stack bloat.
* **Smart Hybrid Memory (RAG + Context Injection):** Automatic pre-fetching of relevant facts before prompt assembly, plus explicit model-driven vector queries when needed.
* **Embedded JS Engine (Boa Runtime):** Safe, deterministic math, data filtering, and scripting executed in a sandboxed JavaScript runtime inside the process.
* **Self-Healing Loop:** Automatic process recovery and prompt correction on invalid model outputs without breaking the main user session.

---

## Official Extensions

* **[osy-system](https://github.com/fuderis/osy-system.git): Local system management agent** —
  Provides system metrics, appearance changing, power & media control, disks & infrastructure management.

---

## Architecture & Ecosystem

Osy utilizes a centralized orchestration model:

<img src="https://raw.githubusercontent.com/fuderis/osy-kernel/main/assets/scheme.png" alt="Scheme" width="100%" />

### Ecosystem of Specialized Rust Crates:

* **Rigging:** Asynchronous inline TUI engine for reactive terminal UI (Markdown, syntax highlighting, Vi/Vim keybindings, and dynamic viewports).
* **AnyLM:** Unified SDK layer for seamless operation across any model provider (OpenAI, Anthropic, Ollama, Local vLLM).
* **Cistern:** High-level async abstraction built on top of Sled (fast KV store) and LanceDB (embedded vector DB).
* **Pearce:** Axum-based networking engine with native UDS client and SSE streaming support.
* **Atoman:** Thread-safe management of asynchronous state and kernel configurations.
* **Boa JS:** Embedded lightweight JavaScript interpreter for deterministic computations without invoking external processes.

---

## Hybrid RAG & Smart Memory

Memory in Osy is split across several managed layers:

| Mechanism | Description |
|---|---|
| **Auto-Trigger Memory** | The kernel scans incoming context and automatically pulls relevant embeddings from LanceDB before sending the request to the LLM. |
| **Explicit Model Pull** | The model can initiate memory calls (`search_fact`, `remember_fact`) on its own if it lacks sufficient data for an accurate response. |
| **Dynamic System Prompts** | User preferences and global instructions are injected into the session in isolation without bloating the dialogue history. |

---

## Comparison: Traditional Frameworks vs. Osy

| Parameter | Traditional Agent Frameworks | Osy Kernel Engine |
|---|---|---|
| **Architecture** | Heavy monolithic Mesh / P2P | Process-isolated Microservices |
| **Context Management** | Polluted by raw JSONs & tool logs | Zero Pollution (Orchestrator sees only final results) |
| **Token Consumption** | Grows exponentially with every call | Strictly bounded & token-scrubbed |
| **Communication** | Heavy HTTP/REST wrappers | Low-latency Unix Domain Sockets (UDS) |
| **Predictability** | High risk of hallucination loops | Deterministic Kernel-managed state machine |
| **Worker Model** | Short-lived per-request spawns | Long-lived persistent IPC micro-workers |

---

## Roadmap

* [x] Long-Term Memory (RAG + Fact Storage).
* [x] Task-Scoped Context & Token Scrubbing.
* [x] Native Process Lifecycle & UDS IPC.
* [x] Embedded JS Engine (Boa Runtime for Isolated Computations).
* [x] Interactive Events with Callback (Confirmation Prompt, Select Menu, etc.).
* [ ] Native Web Search (Obscure integration).

> 💡 **Contributions Welcome:** If you are passionate about low-level Rust systems, deterministic AI
orchestration, or IPC engine design, feel free to open issues, submit pull requests, or reach out!

---

## Quickstart

### Requirements
* **OS:** `Linux`, `macOS`, `BSD`, `Windows`.
* **Rust:** `nightly` toolchain.
* **Dependencies:** `jq` (required by `build.sh`).

### Clone repository

```bash
git clone https://github.com/fuderis/osy-kernel.git && cd osy-kernel
```

### Build from source

Automatically builds and installs Osy CLI on your system.

> For Windows: install `Git Bash` before.

```bash
bash build.sh
```

### Run Osy CLI

```bash
osy --help
```

---

## Licensing & Commercial Usage

This project is distributed under the [**GNU General Public License v3.0**](LICENSE.md).

### Dual Licensing

* **Open Source Use:** You are free to use, modify, and deploy Osy in non-commercial or open-source projects in accordance with GPL-3.0.
* **Commercial License:** To integrate the Osy kernel into proprietary commercial products without disclosing your source code, you must acquire a commercial license.

> For commercial licensing inquiries and enterprise support, please contact the project author: **Bulat Sharipov** ([@fuderis](https://github.com/fuderis) / `synapdrake@ya.ru`).
