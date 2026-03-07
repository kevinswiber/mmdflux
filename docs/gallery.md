# mmdflux gallery

_Generated from commit `6f94ddf` — 110 fixtures_

- [Flowchart](#flowchart) (93)
- [Class](#class) (17)

# Flowchart

## ampersand

`tests/fixtures/flowchart/ampersand.mmd`

**Text**

```text
┌──────────┐    ┌──────────┐
│ Source 1 │    │ Source 2 │
└──────────┘    └──────────┘
        │          │
        └─┐     ┌──┘
          ▼     ▼
         ┌───────┐
         │ Merge │
         └───────┘
          │     │
        ┌─┘     └──┐
        ▼          ▼
┌──────────┐    ┌──────────┐
│ Output 1 │    │ Output 2 │
└──────────┘    └──────────┘
```

<details>
<summary>SVG output</summary>

![ampersand svg](../tests/svg-snapshots/flowchart/ampersand.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Source 1] & B[Source 2] --> C[Merge]
    C --> D[Output 1] & E[Output 2]

```

</details>

## backward_in_subgraph_lr

`tests/fixtures/flowchart/backward_in_subgraph_lr.mmd`

**Text**

```text
┌─────── Group ───────┐
│ ┌──────┐  ┌───────┐ │
│ │ Node │◄►│ Node2 │ │
│ └──────┘ │└───────┘ │
└──────────┼────┼─────┘
           │    │
           └────┘
```

<details>
<summary>SVG output</summary>

![backward_in_subgraph_lr svg](../tests/svg-snapshots/flowchart/backward_in_subgraph_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph sg1[Group]
        direction LR
        A[Node] --> B[Node2]
        B --> A
    end

```

</details>

## backward_in_subgraph

`tests/fixtures/flowchart/backward_in_subgraph.mmd`

**Text**

```text
┌──── Group ────┐
│   ┌──────┐    │
│   │ Node │◄──┐│
│   └──────┘   ││
│       │      ││
│       │      ││
│       ▼      ││
│   ┌───────┐  ││
│   │ Node2 │──┘│
│   └───────┘   │
└───────────────┘
```

<details>
<summary>SVG output</summary>

![backward_in_subgraph svg](../tests/svg-snapshots/flowchart/backward_in_subgraph.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
subgraph sg1[Group]
A[Node] --> B[Node2]
B --> A
end

```

</details>

## backward_loop_lr

`tests/fixtures/flowchart/backward_loop_lr.mmd`

**Text**

```text
                                                      ┌───────────┐             ┌───────────────────┐
                                                      │ IA Schema │────────────►│ Frontend Scaffold │
                                               ┌─────►└───────────┘             └───────────────────┘───┐
┌────────────┐           ┌──────────────┐──────┘                                                        │
│ Narratives │──────────►│ Domain Model │                                                               │
└────────────┘           └──────────────┘─────┐                                                         │
                                              └────►┌─────────────────┐                                 └───►┌───────────────────┐────────────────────►┌────────────────┐                      ┌─────────────────┐
                                                    │ Server Scaffold │───────────────────────┐              │ AI Implementation │     ┌──Fail────────┐│ Quality Checks │─────────Pass────────►│ Production Code │
                                                    └─────────────────┘                       └─────────────►└───────────────────┘◄────┘              └└────────────────┘                      └─────────────────┘
```

<details>
<summary>SVG output</summary>

![backward_loop_lr svg](../tests/svg-snapshots/flowchart/backward_loop_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
flowchart LR
    A[Narratives] --> B[Domain Model]
    B --> C[Server Scaffold]
    B --> D[IA Schema]
    D --> E[Frontend Scaffold]
    C --> F[AI Implementation]
    E --> F
    F --> G[Quality Checks]
    G -->|Fail| F
    G -->|Pass| H[Production Code]

```

</details>

## bidirectional_arrows

`tests/fixtures/flowchart/bidirectional_arrows.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
  ▲
  │
  ▼
┌───┐
│ B │
└───┘
  ▲
  ┆
  ▼
┌───┐
│ C │
└───┘
  ▲
  ┃
  ▼
┌───┐
│ D │
└───┘
```

<details>
<summary>SVG output</summary>

![bidirectional_arrows svg](../tests/svg-snapshots/flowchart/bidirectional_arrows.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A <--> B
    B <-.-> C
    C <==> D

```

</details>

## bidirectional

`tests/fixtures/flowchart/bidirectional.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
  ▲
  │
  ▼
┌───┐
│ B │
└───┘
  ▲
  ┆
  ▼
┌───┐
│ C │
└───┘
  ▲
  ┃
  ▼
┌───┐
│ D │
└───┘
```

<details>
<summary>SVG output</summary>

![bidirectional svg](../tests/svg-snapshots/flowchart/bidirectional.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A <--> B
    B <-.-> C
    C <==> D

```

</details>

## bottom_top

`tests/fixtures/flowchart/bottom_top.mmd`

**Text**

```text
   ┌──────┐
   │ Roof │
   └──────┘
       ▲
       │
       │
 ┌───────────┐
 │ Structure │
 └───────────┘
       ▲
       │
       │
┌────────────┐
│ Foundation │
└────────────┘
```

<details>
<summary>SVG output</summary>

![bottom_top svg](../tests/svg-snapshots/flowchart/bottom_top.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph BT
    Foundation[Foundation] --> Structure[Structure]
    Structure --> Roof[Roof]

```

</details>

## br_line_breaks

`tests/fixtures/flowchart/br_line_breaks.mmd`

**Text**

```text
 ┌───────┐
 │ Hello │
 │ World │
 └───────┘
     │
     │
     │
     ▼
┌────────┐
│ Line 1 │
│ Line 2 │
└────────┘
     │
     │
     │
    yes
    no
     │
     │
     ▼
 ┌───────┐
 │ One   │
 │ Two   │
 │ Three │
 └───────┘
```

<details>
<summary>SVG output</summary>

![br_line_breaks svg](../tests/svg-snapshots/flowchart/br_line_breaks.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Hello<br>World] --> B[Line 1<br/>Line 2]
    B -->|yes<br>no| C[One<BR>Two<BR />Three]

```

</details>

## callgraph_feedback_cycle

`tests/fixtures/flowchart/callgraph_feedback_cycle.mmd`

**Text**

```text
                 ┌──────────────────────┐
                 │ _start s:0.00 t:7.00 │
                 └──────────────────────┘
                             │
                             │
                             ▼
      ┌────────────────────────────────────────────┐
      │ __libc_start_main@GLIBC_2... s:0.00 t:7.00 │
      └────────────────────────────────────────────┘
                             │
                             │
                             ▼
         ┌──────────────────────────────────────┐
         │ __libc_start_call_main s:0.00 t:7.00 │
         └──────────────────────────────────────┘
                             │
                             │
                             ▼
                  ┌────────────────────┐
                  │ main s:0.00 t:7.00 │
                  └────────────────────┘
                             │
                             │
                             ▼
                   ┌───────────────────┐                ┌─────────────────────────┐
                   │ fn9 s:0.00 t:7.00 │                │ __clone3 s:0.00 t:49.00 │
                   └───────────────────┘                └─────────────────────────┘
                             │                                       │
                             │                                       │
                             ▼                                       ▼
                  ┌────────────────────┐              ┌─────────────────────────────┐
                  │ fn10 s:0.00 t:7.00 │              │ start_thread s:0.00 t:49.00 │
                  └────────────────────┘              └─────────────────────────────┘
                             │                                       │
                             │                                       │
                             ▼                                       ▼
                  ┌────────────────────┐                 ┌───────────────────────┐
                  │ fn11 s:0.00 t:7.00 │                 │ fn1... s:0.00 t:48.00 │
                  └────────────────────┘                 └───────────────────────┘
                             │                            │             │
                   ┌─────────┘        ┌───────────────────┘             │
                   ▼                  ▼                                 │
                  ┌────────────────────┐                                │
                  │ fn7 s:0.00 t:46.00 │                                │
                  └────────────────────┘                                │
                   │                  │                                 │
                  ┌┘                  └─┐                               │
                  ▼                     ▼                               │
   ┌────────────────────┐         ┌───────────────────┐                 │
   │ fn8 s:0.00 t:39.00 │         │ fn1 s:0.00 t:7.00 │                 │
   └────────────────────┘         └───────────────────┘                 │
              │                             │                           │
              │                             │                           │
              ▼                             ▼                           │
┌──────────────────────────┐    ┌──────────────────────┐                │
│ fn8_in... s:1.00 t:39.00 │    │ fn1... s:0.00 t:7.00 │                │
└──────────────────────────┘    └──────────────────────┘                │
                      │                                                 │
                      └───────────┐            ┌────────────────────────┘
                                  ▼            ▼
                                 ┌────────────────────┐
                                 │ fn2 s:1.00 t:47.00 │
                                 └────────────────────┘
                                  │                  │
                                  │                  └─┐
                                  ▼                    ▼
                ┌──────────────────────────┐    ┌────────────────────┐
                │ fn3_co... s:0.00 t:31.00 │    │ fn6 s:1.00 t:15.00 │
                └──────────────────────────┘─┐  └────────────────────┘
                 │                      ┌────┼──────────┘           │
                 └──┐                   │    └───┐                  └┐
                    ▼                   ▼        ▼                   ▼
                   ┌─────────────────────┐      ┌─────────────────────┐
                   │ fn4 s:27.00 t:27.00 │      │ fn5 s:18.00 t:18.00 │
                   └─────────────────────┘      └─────────────────────┘
```

<details>
<summary>SVG output</summary>

![callgraph_feedback_cycle svg](../tests/svg-snapshots/flowchart/callgraph_feedback_cycle.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
  n0["__clone3 s:0.00 t:49.00"]
  n1["start_thread s:0.00 t:49.00"]
  n0 --> n1
  n2["fn1... s:0.00 t:48.00"]
  n1 --> n2
  n3["fn2 s:1.00 t:47.00"]
  n2 --> n3
  n4["fn3_co... s:0.00 t:31.00"]
  n3 --> n4
  n5["fn4 s:27.00 t:27.00"]
  n4 --> n5
  n6["fn5 s:18.00 t:18.00"]
  n4 --> n6
  n7["fn6 s:1.00 t:15.00"]
  n3 --> n7
  n7 --> n5
  n7 --> n6
  n8["fn7 s:0.00 t:46.00"]
  n2 --> n8
  n9["fn8 s:0.00 t:39.00"]
  n8 --> n9
  n10["fn8_in... s:1.00 t:39.00"]
  n9 --> n10
  n10 --> n3
  n11["fn1 s:0.00 t:7.00"]
  n8 --> n11
  n12["fn1... s:0.00 t:7.00"]
  n11 --> n12
  n13["_start s:0.00 t:7.00"]
  n14["__libc_start_main@GLIBC_2... s:0.00 t:7.00"]
  n13 --> n14
  n15["__libc_start_call_main s:0.00 t:7.00"]
  n14 --> n15
  n16["main s:0.00 t:7.00"]
  n15 --> n16
  n17["fn9 s:0.00 t:7.00"]
  n16 --> n17
  n18["fn10 s:0.00 t:7.00"]
  n17 --> n18
  n19["fn11 s:0.00 t:7.00"]
  n18 --> n19
  n19 --> n8

```

</details>

## chain

`tests/fixtures/flowchart/chain.mmd`

**Text**

```text
┌────────┐
│ Step 1 │
└────────┘
     │
     │
     ▼
┌────────┐
│ Step 2 │
└────────┘
     │
     │
     ▼
┌────────┐
│ Step 3 │
└────────┘
     │
     │
     ▼
┌────────┐
│ Step 4 │
└────────┘
```

<details>
<summary>SVG output</summary>

![chain svg](../tests/svg-snapshots/flowchart/chain.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Step 1] --> B[Step 2] --> C[Step 3] --> D[Step 4]

```

</details>

## ci_pipeline

`tests/fixtures/flowchart/ci_pipeline.mmd`

**Text**

```text
                                                                                                                                                          ┌─────────────┐
                                                                                                                          ┌───────────staging────┐        │ Staging Env │
                                                                                                                          │                      └───────►└─────────────┘
┌──────────┐                 ┌───────┐                ┌───────────┐              ┌────────────┐                ┌─────────┐┘
│ Git Push │────────────────►│ Build │───────────────►│ Run Tests │─────────────►│ Lint Check │───────────────►< Deploy? >
└──────────┘                 └───────┘                └───────────┘              └────────────┘                └─────────┘┐
                                                                                                                          │                     ┌───────►┌────────────┐
                                                                                                                          └─────────────────────┘        │ Production │
                                                                                                                                    production           └────────────┘
```

<details>
<summary>SVG output</summary>

![ci_pipeline svg](../tests/svg-snapshots/flowchart/ci_pipeline.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    Push[Git Push] --> Build[Build]
    Build --> Test[Run Tests]
    Test --> Lint[Lint Check]
    Lint --> Deploy{Deploy?}
    Deploy -->|staging| Staging[Staging Env]
    Deploy -->|production| Prod[Production]

```

</details>

## compat_class_annotation

`tests/fixtures/flowchart/compat_class_annotation.mmd`

**Text**

```text
     ┌───────┐
     │ Start │
     └───────┘
         │
         │
         │
         │
         ▼
   ┌──────────┐
   < Decision >
   └──────────┘
  ┌─┘        └──┐
  │             │
  │             │
  │             │
 Yes           No
  │             │
  │             │
  │             │
  ▼             ▼
┌───┐         ┌───┐
│ C │         │ D │
└───┘         └───┘
```

<details>
<summary>SVG output</summary>

![compat_class_annotation svg](../tests/svg-snapshots/flowchart/compat_class_annotation.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start]:::highlight --> B{Decision}
    B -->|Yes| C:::success
    B -->|No| D:::error
    classDef highlight fill:#ff0
    classDef success fill:#0f0
    classDef error fill:#f00

```

</details>

## compat_directive

`tests/fixtures/flowchart/compat_directive.mmd`

**Text**

```text
         ┌───────┐
         │ Start │
         └───────┘
             │
             │
             │
             │
             ▼
       ┌──────────┐
       < Decision >
       └──────────┘
     ┌──┘        └───┐
     │               │
     │               │
     │               │
    Yes             No
     │               │
     │               │
     │               │
     ▼               ▼
┌─────────┐       ┌─────┐
│ Process │       │ End │
└─────────┘       └─────┘
```

<details>
<summary>SVG output</summary>

![compat_directive svg](../tests/svg-snapshots/flowchart/compat_directive.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
%%{init: {"theme": "dark", "flowchart": {"curve": "basis"}}}%%
graph TD
    A[Start] --> B{Decision}
    B -->|Yes| C[Process]
    B -->|No| D[End]

```

</details>

## compat_frontmatter

`tests/fixtures/flowchart/compat_frontmatter.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
  │
  │
  ▼
┌───┐
│ B │
└───┘
  │
  │
  ▼
┌───┐
│ C │
└───┘
```

<details>
<summary>SVG output</summary>

![compat_frontmatter svg](../tests/svg-snapshots/flowchart/compat_frontmatter.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
---
config:
  theme: dark
---
graph TD
    A --> B --> C

```

</details>

## compat_hyphenated_ids

`tests/fixtures/flowchart/compat_hyphenated_ids.mmd`

**Text**

```text
  ┌───────┐
  │ Start │
  └───────┘
      │
      │
      ▼
┌───────────┐
│ Process A │
└───────────┘
      │
      │
      ▼
  ┌───────┐
  < Check >
  └───────┘
      │
      │
      │
     ok
      │
      ▼
  ┌──────┐
  │ Done │
  └──────┘
```

<details>
<summary>SVG output</summary>

![compat_hyphenated_ids svg](../tests/svg-snapshots/flowchart/compat_hyphenated_ids.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    start-node[Start] --> process-1[Process A]
    process-1 --> decision-point{Check}
    decision-point -->|ok| end-node[Done]

```

</details>

## compat_invisible_edge

`tests/fixtures/flowchart/compat_invisible_edge.mmd`

**Text**

```text
   ┌───┐
   │ A │
   └───┘
    │ └─┐
   ┌┘   │
   ▼    │
┌───┐   │
│ B │   │
└───┘ ┌─┘
      │
      │
      ▼
   ┌───┐
   │ C │
   └───┘
```

<details>
<summary>SVG output</summary>

![compat_invisible_edge svg](../tests/svg-snapshots/flowchart/compat_invisible_edge.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A --> B
    A --> C
    B ~~~ C

```

</details>

## compat_kitchen_sink

`tests/fixtures/flowchart/compat_kitchen_sink.mmd`

**Text**

```text
             ┌───────┐
             │ Start │
             └───────┘
                 │
                 │
                 │
                 │
                 ▼
          ┌─────────────┐
          < Check Input >
          └─────────────┘
      ┌────┘           └────┐
      │                     │
      │                     │
      │                     │
    valid                invalid
      │                     │
      │                     │
      │                     │
      ▼                     ▼
┌───────────┐           ┌───────┐
│ process-A │           │ Error │
└───────────┘           └───────┘
        │                 │
        └─────┐    ┌──────┘
              │    │
              │    │
              ▼    ▼
             ┌──────┐
             │ Done │
             └──────┘
```

<details>
<summary>SVG output</summary>

![compat_kitchen_sink svg](../tests/svg-snapshots/flowchart/compat_kitchen_sink.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
---
config:
  theme: default
---
%%{init: {"flowchart": {"curve": "basis"}}}%%
graph TD
    start-node[Start] --> check-1{Check Input}
    check-1 -->|valid| process-A:::success
    check-1 -->|invalid| error-1[Error]:::error
    process-A --> end-node[Done]
    error-1 --> end-node
    style start-node fill:#f9f
    classDef success fill:#0f0
    classDef error fill:#f00

```

</details>

## compat_no_direction

`tests/fixtures/flowchart/compat_no_direction.mmd`

**Text**

```text
┌───────┐
│ Start │
└───────┘
    │
    │
    ▼
 ┌─────┐
 │ End │
 └─────┘
```

<details>
<summary>SVG output</summary>

![compat_no_direction svg](../tests/svg-snapshots/flowchart/compat_no_direction.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph
    A[Start] --> B[End]

```

</details>

## compat_numeric_ids

`tests/fixtures/flowchart/compat_numeric_ids.mmd`

**Text**

```text
┌───────┐    ┌────────┐     ┌───────┐
│ First │───►│ Second │────►│ Third │
└───────┘    └────────┘     └───────┘
```

<details>
<summary>SVG output</summary>

![compat_numeric_ids svg](../tests/svg-snapshots/flowchart/compat_numeric_ids.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    1[First] --> 2[Second]
    2 --> 3[Third]

```

</details>

## complex

`tests/fixtures/flowchart/complex.mmd`

**Text**

```text
                           ┌───────┐
                           │ Input │◄────────────────────────────yes
                           └───────┘                               │
                            │                                      │
                            └──┐                                   │
                               │                                   │
                               │                                   │
                               ▼                                   │
                         ┌──────────┐                              │
                         < Validate >                              │
                         └──────────┘                              │
                  ┌───────┘        └───────────────────┐           │
                  │                                    │           │
               invalid                               valid         │
                  │                                    │           │
                  ▼                                    ▼           │
          ╭───────────────╮                       ┌─────────┐      │
          │ Error Handler │                       │ Process │      │
          ╰───────────────╯                       └─────────┘      │
           ┆             ┃                             │           │
        ┌┄┄┘             ┗━━━┓                         └───────┐   │
        ┆                    ┃                                 │   │
        ┆                    ┃                                 │   │
        ▼                    ▼                                 ▼   │
┌───────────┐           ┌──────────────┐           ┌────────────┐  │
│ Log Error │           │ Notify Admin │           < More Data? >──┘
└───────────┘           └──────────────┘           └────────────┘
          │                │                           │
          └───┐       ┌────┘                           │
              │       │                                │
              │       │                                │
              ▼       ▼                                │
             ┌─────────┐                              no
             │ Cleanup │                               │
             └─────────┘───┐                           │
                           │      ┌────────────────────┘
                           │      │
                           │      │
                           │      │
                           ▼      ▼
                          ┌────────┐
                          │ Output │
                          └────────┘
```

<details>
<summary>SVG output</summary>

![complex svg](../tests/svg-snapshots/flowchart/complex.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    %% Complex diagram with multiple features
    A[Input] --> B{Validate}
    B -->|valid| C[Process]
    B -->|invalid| D(Error Handler)
    C --> E{More Data?}
    E -->|yes| A
    E -->|no| F[Output]
    D -.-> G[Log Error]
    D ==> H[Notify Admin]
    G & H --> I[Cleanup]
    I --> F

```

</details>

## criss_cross

`tests/fixtures/flowchart/criss_cross.mmd`

**Text**

```text
     ┌───┐
     │ A │
     └───┘
      │ │
   ┌──┘ └─┐
   ▼      ▼
┌───┐    ┌───┐
│ B │    │ C │
└───┘──┐ └───┘
 │ ┌───┼──┘ │
 │ │   └──┐ │
 ▼ ▼      ▼ ▼
┌───┐    ┌───┐
│ D │    │ E │
└───┘    └───┘
```

<details>
<summary>SVG output</summary>

![criss_cross svg](../tests/svg-snapshots/flowchart/criss_cross.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
  A --> B
  A --> C
  C --> D
  B --> D
  B --> E
  C --> E

```

</details>

## cross_circle_arrows

`tests/fixtures/flowchart/cross_circle_arrows.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
  │
  │
  x
┌───┐
│ B │
└───┘
  │
  │
  o
┌───┐
│ C │
└───┘
  x
  │
  x
┌───┐
│ D │
└───┘
  o
  │
  o
┌───┐
│ E │
└───┘
```

<details>
<summary>SVG output</summary>

![cross_circle_arrows svg](../tests/svg-snapshots/flowchart/cross_circle_arrows.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A --x B
    B --o C
    C x--x D
    D o--o E

```

</details>

## crossing_minimize

`tests/fixtures/flowchart/crossing_minimize.mmd`

**Text**

```text
                           ┌───────┐
                           │ Input │◄────────────────────────────yes
                           └───────┘                               │
                            │                                      │
                            └──┐                                   │
                               │                                   │
                               │                                   │
                               ▼                                   │
                         ┌──────────┐                              │
                         < Validate >                              │
                         └──────────┘                              │
                  ┌───────┘        └───────────────────┐           │
                  │                                    │           │
               invalid                               valid         │
                  │                                    │           │
                  ▼                                    ▼           │
          ╭───────────────╮                       ┌─────────┐      │
          │ Error Handler │                       │ Process │      │
          ╰───────────────╯                       └─────────┘      │
           ┆             ┃                             │           │
        ┌┄┄┘             ┗━━━┓                         └───────┐   │
        ┆                    ┃                                 │   │
        ┆                    ┃                                 │   │
        ▼                    ▼                                 ▼   │
┌───────────┐           ┌──────────────┐           ┌────────────┐  │
│ Log Error │           │ Notify Admin │           < More Data? >──┘
└───────────┘           └──────────────┘           └────────────┘
          │                │                           │
          └───┐       ┌────┘                           │
              │       │                                │
              │       │                                │
              ▼       ▼                                │
             ┌─────────┐                              no
             │ Cleanup │                               │
             └─────────┘───┐                           │
                           │      ┌────────────────────┘
                           │      │
                           │      │
                           │      │
                           ▼      ▼
                          ┌────────┐
                          │ Output │
                          └────────┘
```

<details>
<summary>SVG output</summary>

![crossing_minimize svg](../tests/svg-snapshots/flowchart/crossing_minimize.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
flowchart TB
    A["Input"] --> B{"Validate"}
    B -- valid --> C["Process"]
    B -- invalid --> D("Error Handler")
    C --> E{"More Data?"}
    E -- yes --> A
    D -.-> G["Log Error"]
    D ==> H["Notify Admin"]
    G --> I["Cleanup"]
    H --> I
    I --> F["Output"]
    E -- no --> F

```

</details>

## decision

`tests/fixtures/flowchart/decision.mmd`

**Text**

```text
      ┌───────┐
      │ Start │◄─────────────┐
      └───────┘              │
       │                     │
       └─┐                   │
         │                   │
         │                   │
         ▼                   │
┌────────────────┐           │
< Is it working? >           │
└────────────────┘           │
 └─────┐        │            │
       │        │            │
       │        │            │
       │        │            │
      Yes      No            │
       │        │            │
       │        └────┐       │
       │             │       │
       ▼             ▼       │
  ┌────────┐        ┌───────┐│
  │ Great! │        │ Debug │┘
  └────────┘        └───────┘
```

<details>
<summary>SVG output</summary>

![decision svg](../tests/svg-snapshots/flowchart/decision.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B{Is it working?}
    B -->|Yes| C[Great!]
    B -->|No| D[Debug]
    D --> A

```

</details>

## diamond_backward

`tests/fixtures/flowchart/diamond_backward.mmd`

**Text**

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     │
     ▼
 ┌───────┐
 < Check >◄──┐
 └───────┘   │
     │       │
     │       │
     ▼       │
┌─────────┐  │
│ Process │──┘
└─────────┘
```

<details>
<summary>SVG output</summary>

![diamond_backward svg](../tests/svg-snapshots/flowchart/diamond_backward.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B{Check}
    B --> C[Process]
    C --> B

```

</details>

## diamond_fan_out

`tests/fixtures/flowchart/diamond_fan_out.mmd`

**Text**

```text
           ┌──────────┐
           < Decision >
           └──────────┘
            │   │    │
       ┌────┘   └┐   └──────┐
       ▼         ▼          ▼
┌──────┐    ┌────────┐     ┌───────┐
│ Left │    │ Center │     │ Right │
└──────┘    └────────┘     └───────┘
```

<details>
<summary>SVG output</summary>

![diamond_fan_out svg](../tests/svg-snapshots/flowchart/diamond_fan_out.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A{Decision} --> B[Left]
    A --> C[Center]
    A --> D[Right]

```

</details>

## diamond_fan

`tests/fixtures/flowchart/diamond_fan.mmd`

**Text**

```text
      ┌───────┐
      │ Start │
      └───────┘
       │     │
      ┌┘     └─┐
      ▼        ▼
┌──────┐    ┌───────┐
│ Left │    │ Right │
└──────┘    └───────┘
      │        │
      └─┐   ┌──┘
        ▼   ▼
       ┌─────┐
       │ End │
       └─────┘
```

<details>
<summary>SVG output</summary>

![diamond_fan svg](../tests/svg-snapshots/flowchart/diamond_fan.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B[Left]
    A --> C[Right]
    B --> D[End]
    C --> D

```

</details>

## direction_override

`tests/fixtures/flowchart/direction_override.mmd`

**Text**

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     └─┐
       │
       │
       │
       │
       │
┌──────┼─ Horizontal Section ────────┐
│      ▼                             │
│ ┌────────┐  ┌────────┐  ┌────────┐ │
│ │ Step 1 │─►│ Step 2 │─►│ Step 3 │ │
│ └────────┘  └────────┘  └────────┘ │
│                     ┌────┘         │
└─────────────────────┼──────────────┘
                      │
                      │
                      │
                      │
                      │
                      │
                      ▼
                 ┌─────┐
                 │ End │
                 └─────┘
```

<details>
<summary>SVG output</summary>

![direction_override svg](../tests/svg-snapshots/flowchart/direction_override.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph sg1[Horizontal Section]
        direction LR
        A[Step 1] --> B[Step 2] --> C[Step 3]
    end
    Start --> A
    C --> End

```

</details>

## double_skip

`tests/fixtures/flowchart/double_skip.mmd`

**Text**

```text
          ┌───────┐
          │ Start │──┐
          └───────┘  │
           │  │      │
       ┌───┘  │      │
       ▼      │      │
┌────────┐    │      │
│ Step 1 │    │      │
└────────┘   ┌┘      │
      │      │       │
      │      │       │
      ▼      ▼       │
     ┌────────┐      │
     │ Step 2 │      │
     └────────┘      │
           │         │
           └┐        │
            ▼        │
           ┌─────┐   │
           │ End │◄──┘
           └─────┘
```

<details>
<summary>SVG output</summary>

![double_skip svg](../tests/svg-snapshots/flowchart/double_skip.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B[Step 1]
    B --> C[Step 2]
    C --> D[End]
    A --> C
    A --> D

```

</details>

## edge_styles

`tests/fixtures/flowchart/edge_styles.mmd`

**Text**

```text
 ┌───────┐    ┌────────┐    ┌───────┐    ┌──────┐
 │ Solid │    │ Dotted │    │ Thick │    │ Open │
 └───────┘    └────────┘    └───────┘    └──────┘
     │            ┆            ┃            │
     │            ┆            ┃            │
     ▼            ▼            ▼            │
┌────────┐    ┌───────┐    ┌───────┐    ┌──────┐
│ Normal │    │ Arrow │    │ Arrow │    │ Line │
└────────┘    └───────┘    └───────┘    └──────┘
```

<details>
<summary>SVG output</summary>

![edge_styles svg](../tests/svg-snapshots/flowchart/edge_styles.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Solid] --> B[Normal]
    C[Dotted] -.-> D[Arrow]
    E[Thick] ==> F[Arrow]
    G[Open] --- H[Line]

```

</details>

## external_node_subgraph

`tests/fixtures/flowchart/external_node_subgraph.mmd`

**Text**

```text
                         ┌───────────────┐
                         │ Load Balancer │
                         └───────────────┘
                  ┌───────┘             └───────┐
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
                  │                             │
┌─────────────────┼────────── Cloud ────────────┼────────────────┐
│     ┌─── US West┼Region ───┐      ┌─── US East┼Region ───┐     │
│     │           ▼          │      │           ▼          │     │
│     │    ┌────────────┐    │      │    ┌────────────┐    │     │
│     │    │ Web Server │    │      │    │ Web Server │    │     │
│     │    └────────────┘    │      │    └────────────┘    │     │
│     │           │          │      │           │          │     │
│     │           │          │      │           │          │     │
│     │           ▼          │      │           ▼          │     │
│     │    ┌────────────┐    │      │    ┌────────────┐    │     │
│     │    │ App Server │    │      │    │ App Server │    │     │
│     │    └────────────┘    │      │    └────────────┘    │     │
│     └──────────────────────┘      └──────────────────────┘     │
└────────────────────────────────────────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![external_node_subgraph svg](../tests/svg-snapshots/flowchart/external_node_subgraph.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
  subgraph Cloud
    subgraph us-east [US East Region]
      A[Web Server] --> B[App Server]
    end
    subgraph us-west [US West Region]
      C[Web Server] --> D[App Server]
    end
  end
  E[Load Balancer] --> A
  E --> C

```

</details>

## fan_in_backward_channel_conflict

`tests/fixtures/flowchart/fan_in_backward_channel_conflict.mmd`

**Text**

```text
┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐
│ Source1 │    │ Source2 │    │ Source3 │    │ Source4 │    │ Source5 │
└─────────┘    └─────────┘    └─────────┘    └─────────┘    └─────────┘
          │             │         │          │              │
          └─────────────┴─────┬┐ ┌┘┌─┬───────┴──────────────┘
                              ▼▼ ▼ ▼ ▼
                             ┌────────┐
                             │ Target │◄┐
                             └────────┘ │
                                  │     │
                                  │     │
                                  ▼     │
                              ┌──────┐  │
                              │ Sink │──┘
                              └──────┘
```

<details>
<summary>SVG output</summary>

![fan_in_backward_channel_conflict svg](../tests/svg-snapshots/flowchart/fan_in_backward_channel_conflict.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    P1[Source1] --> B[Target]
    P2[Source2] --> B
    P3[Source3] --> B
    P4[Source4] --> B
    P5[Source5] --> B
    B --> Loop[Sink]
    Loop --> B

```

</details>

## fan_in_lr

`tests/fixtures/flowchart/fan_in_lr.mmd`

**Text**

```text
┌───────┐
│ Src A │
└───────┘─┐
          │
          │
          │
┌───────┐ └─►┌────────┐
│ Src B │───►│ Target │
└───────┘ ┌─►└────────┘
          │
          │
          │
┌───────┐─┘
│ Src C │
└───────┘
```

<details>
<summary>SVG output</summary>

![fan_in_lr svg](../tests/svg-snapshots/flowchart/fan_in_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    A[Src A] --> D[Target]
    B[Src B] --> D
    C[Src C] --> D

```

</details>

## fan_in

`tests/fixtures/flowchart/fan_in.mmd`

**Text**

```text
┌──────────┐    ┌──────────┐    ┌──────────┐
│ Source A │    │ Source B │    │ Source C │
└──────────┘    └──────────┘    └──────────┘
          │          │           │
          └──────┐  ┌┘  ┌────────┘
                 ▼  ▼   ▼
                ┌────────┐
                │ Target │
                └────────┘
```

<details>
<summary>SVG output</summary>

![fan_in svg](../tests/svg-snapshots/flowchart/fan_in.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Source A] --> D[Target]
    B[Source B] --> D
    C[Source C] --> D

```

</details>

## fan_out

`tests/fixtures/flowchart/fan_out.mmd`

**Text**

```text
                ┌────────┐
                │ Source │
                └────────┘
                 │  │   │
          ┌──────┘  └┐  └────────┐
          ▼          ▼           ▼
┌──────────┐    ┌──────────┐    ┌──────────┐
│ Target A │    │ Target B │    │ Target C │
└──────────┘    └──────────┘    └──────────┘
```

<details>
<summary>SVG output</summary>

![fan_out svg](../tests/svg-snapshots/flowchart/fan_out.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Source] --> B[Target A]
    A --> C[Target B]
    A --> D[Target C]

```

</details>

## five_fan_in_diamond

`tests/fixtures/flowchart/five_fan_in_diamond.mmd`

**Text**

```text
┌───┐     ┌───┐     ┌───┐    ┌───┐     ┌───┐
│ A │     │ B │     │ C │    │ D │     │ E │
└───┘     └───┘     └───┘    └───┘     └───┘
    │         │       │      │         │
    └─────────┴───┬┐ ┌┘┌─┬───┴─────────┘
                  ▼▼ ▼ ▼ ▼
                 ┌────────┐
                 < Target >
                 └────────┘
```

<details>
<summary>SVG output</summary>

![five_fan_in_diamond svg](../tests/svg-snapshots/flowchart/five_fan_in_diamond.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[A] --> F{Target}
    B[B] --> F
    C[C] --> F
    D[D] --> F
    E[E] --> F

```

</details>

## five_fan_in_lr

`tests/fixtures/flowchart/five_fan_in_lr.mmd`

**Text**

```text
┌───┐
│ A │
└───┘──┐
       │
       │
       │
┌───┐  │
│ B │  │
└───┘──┤
       │
       │
       │
┌───┐  └─►┌────────┐
│ C │──┬─►│ Target │
└───┘  ├─►└────────┘
       │
       │
       │
┌───┐──┤
│ D │  │
└───┘  │
       │
       │
       │
┌───┐──┘
│ E │
└───┘
```

<details>
<summary>SVG output</summary>

![five_fan_in_lr svg](../tests/svg-snapshots/flowchart/five_fan_in_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    A[A] --> F[Target]
    B[B] --> F
    C[C] --> F
    D[D] --> F
    E[E] --> F

```

</details>

## five_fan_in

`tests/fixtures/flowchart/five_fan_in.mmd`

**Text**

```text
┌───┐     ┌───┐     ┌───┐    ┌───┐     ┌───┐
│ A │     │ B │     │ C │    │ D │     │ E │
└───┘     └───┘     └───┘    └───┘     └───┘
    │         │       │      │         │
    └─────────┴───┬┐ ┌┘┌─┬───┴─────────┘
                  ▼▼ ▼ ▼ ▼
                 ┌────────┐
                 │ Target │
                 └────────┘
```

<details>
<summary>SVG output</summary>

![five_fan_in svg](../tests/svg-snapshots/flowchart/five_fan_in.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[A] --> F[Target]
    B[B] --> F
    C[C] --> F
    D[D] --> F
    E[E] --> F

```

</details>

## five_fan_out_diamond

`tests/fixtures/flowchart/five_fan_out_diamond.mmd`

**Text**

```text
                                ┌────────┐
                                < Source >
                                └────────┘
                                 ││ │ │ │
           ┌─────────────┬───────┴┘ └┐└─┴────────┬──────────────┐
           ▼             ▼           ▼           ▼              ▼
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│ Target A │    │ Target B │    │ Target C │    │ Target D │    │ Target E │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
```

<details>
<summary>SVG output</summary>

![five_fan_out_diamond svg](../tests/svg-snapshots/flowchart/five_fan_out_diamond.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
      A{Source} --> B[Target A]
      A --> C[Target B]
      A --> D[Target C]
      A --> E[Target D]
      A --> F[Target E]
```

</details>

## five_fan_out_lr

`tests/fixtures/flowchart/five_fan_out_lr.mmd`

**Text**

```text
               ┌──────────┐
               │ Target A │
            ┌─►└──────────┘
            │
            │
            │
            │  ┌──────────┐
            │  │ Target B │
            ├─►└──────────┘
            │
            │
            │
┌────────┐──┘  ┌──────────┐
│ Source │──┬─►│ Target C │
└────────┘──┤  └──────────┘
            │
            │
            │
            ├─►┌──────────┐
            │  │ Target D │
            │  └──────────┘
            │
            │
            │
            └─►┌──────────┐
               │ Target E │
               └──────────┘
```

<details>
<summary>SVG output</summary>

![five_fan_out_lr svg](../tests/svg-snapshots/flowchart/five_fan_out_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
      A[Source] --> B[Target A]
      A --> C[Target B]
      A --> D[Target C]
      A --> E[Target D]
      A --> F[Target E]

```

</details>

## five_fan_out

`tests/fixtures/flowchart/five_fan_out.mmd`

**Text**

```text
                                ┌────────┐
                                │ Source │
                                └────────┘
                                 ││ │ │ │
           ┌─────────────┬───────┴┘ └┐└─┴────────┬──────────────┐
           ▼             ▼           ▼           ▼              ▼
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│ Target A │    │ Target B │    │ Target C │    │ Target D │    │ Target E │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
```

<details>
<summary>SVG output</summary>

![five_fan_out svg](../tests/svg-snapshots/flowchart/five_fan_out.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
      A[Source] --> B[Target A]
      A --> C[Target B]
      A --> D[Target C]
      A --> E[Target D]
      A --> F[Target E]
```

</details>

## flowchart_code_flow

`tests/fixtures/flowchart/flowchart_code_flow.mmd`

**Text**

```text
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               ┌─────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               │ User Input Text │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               └─────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               ┌─────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               < Detection Phase >
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               └─────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                │       │       │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  ┌─────────────┘       │       └──────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  │                     │                      │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  │                     │                      │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  ▼                     ▼                      ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                ┌───────────────────────┐    ┌───────────────────────┐    ┌───────────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                │ flowDetector.ts       │    │ flowDetector-v2.ts    │    │ elk/detector.ts       │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                │ detector(txt, config) │    │ detector(txt, config) │    │ detector(txt, config) │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                └───────────────────────┘    └───────────────────────┘    └───────────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   Checks /^\s*graph/        Checks /^\s*flowchart/     Checks /^\s*flowchart-elk/
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            ▼                           ▼                            ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  ┌───────────────────┐        ┌────────────────┐             ┌─────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  < Legacy Flowchart? >        < New Flowchart? >             < ELK Layout? >
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  └───────────────────┘        └────────────────┘             └─────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           Yes                         Yes                          Yes
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │                           │                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            └──────────────────┐        │        ┌───────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               │        │        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               │        │        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               ▼        ▼        ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              ┌───────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              │ loader() function │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              └───────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               ┌────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               │ flowDiagram.ts │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               │ diagram object │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               └────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             ┌────────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             < Diagram Components >
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             └────────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              │   │    │    │    │
                                                                                                             ┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┴───┘    └────┴────┼─────────────────────────┬───────────────────────────────────────────────────────────────────────┐
                                                                                                             │                                                                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                         │                                                                       │
                                                                                                             │                                                                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                         │                                                                       │
                                                                                                             │                                                                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                         │                                                                       │
                                                                                                             │                                                                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ▼                         ▼                                                                       ▼
                                                                                        ┌────────────────────┐                                                                                                                                                                                                                                                                                                                                                       ┌──────────────────┐                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ┌───────────────────────────────────┐    ┌────────────────────┐                                                   ┌────────────────────────────┐
                                                                                        │ parser: flowParser │                                                                                                                                                                                                                                                                                                                                                       │ db: new FlowDB() │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │ renderer: flowRenderer-v3-unified │    │ styles: flowStyles │                                                   │ init: (cnf: MermaidConfig) │
                                                                                        └────────────────────┘                                                                                                                                                                                                                                                                                                                                                       └──────────────────┘                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        └───────────────────────────────────┘    └────────────────────┘                                                   └────────────────────────────┘
                                                                                         │                  │                                                                                                                                                                                                                                                                                                                                                                  │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                              │                                                                               │
                                                             ┌───────────────────────────┘                  └───────────────────────────────────────┐                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                              │                                                                               │
                                                             │                                                                                      │                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                              │                                                                               │
                                                             │                                                                                      │                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                              │                                                                               │
                                                             │                                                                                      │                                                                                                                                                                                                                                                                                                                          │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                              │                                                                               │
                                                             ▼                                                                                      ▼                                                                                                                                                                                                                                                                                                                          ▼                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   ▼                              ▼                                                                               │
                                        ┌──────────────────────┐                                                                                    ┌──────────────────┐                                                                                                                                                                                                                                                                                               ┌──────────────┐                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            ┌───────────────────────────────┐    ┌────────────────────┐                                                                    ▼
                                        │ parser/flowParser.ts │                                                                                    │ types.ts         │                                                                                                                                                                                                                                                                                               │ flowDb.ts    │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │ flowRenderer-v3-unified.ts    │    │ styles.ts          │                                                         ┌─────────────────────┐
                                        │ newParser.parse(src) │                                                                                    │ Type Definitions │                                                                                                                                                                                                                                                                                               │ FlowDB class │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            │ draw(text, id, version, diag) │    │ getStyles(options) │                                                         │ Configuration Setup │
                                        └──────────────────────┘                                                                                    └──────────────────┘                                                                                                                                                                                                                                                                                               └──────────────┘                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            └───────────────────────────────┘    └────────────────────┘                                                         └─────────────────────┘
                                                    │                                                                                                │   │   │   │    │                                                                                                                                                                                                                                                                                                 │            │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             │                              │                                                                     │     │      │      │
                                                    │                          ┌──────────────────────────┬─────────────────────────┬────────────────┴───┴───┘┌──┘    └──────────────────┐                                                                                                                                                                                                                                                              ┌───────────────┘            └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐                                                                              │                              │                               ┌─────────────────────────┬───────────┴─────┘      └┐     └──────────────────┐
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          │                                                                                                                                                                                                                                                              │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              │                              │                               │                         │                         │                        │
                                                    │                          │                          │                         │                         │                          ▼                                                                                                                                                                                                                                                              ▼                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           │                                                                              ▼                              ▼                               │                         │                         │                        │
                                            Preprocesses src                   ▼                          ▼                         ▼                         ▼                 ┌─────────────────────┐                                                                                                                                                                                                                                 ┌───────────────────────┐                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                                                                 ┌────────────────────────┐    ┌───────────────────────────┐                 ▼                         ▼                         ▼                        ▼
                                                    │         ┌──────────────────────┐     ┌────────────────────┐      ┌─────────────────────┐    ┌────────────────────────┐    │ FlowVertexTypeParam │                                                                                                                                                                                                                                 │ constructor()         │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   ┌────────────┐                                                    │ Initialize rendering   │    │ FlowChartStyleOptions     │    ┌──────────────────────┐    ┌─────────────────────┐      ┌───────────────┐        ┌───────────────────┐
                                                    │         │ FlowVertex interface │     │ FlowEdge interface │      │ FlowClass interface │    │ FlowSubGraph interface │    │ Shape types         │                                                                                                                                                                                                                                 │ - Initialize counters │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │ Test Files │                                                    │ - getConfig()          │    │ - arrowheadColor, border2 │    │ cnf.flowchart config │    │ arrowMarkerAbsolute │      │ layout config │        │ setConfig() calls │
                                                    │         └──────────────────────┘     └────────────────────┘      └─────────────────────┘    └────────────────────────┘    └─────────────────────┘                                                                                                                                                                                                                                 │ - Bind methods        │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   └────────────┘                                                    │ - handle securityLevel │    │ - clusterBkg, mainBkg     │    └──────────────────────┘    └─────────────────────┘      └───────────────┘        └───────────────────┘
                                                    │                                                                                                                                                                                                                                                                                                                                                                                   │ - Setup toolTips      │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │    │     │                                                     │ - getDiagramElement()  │    │ - fontFamily, textColor   │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                   │ - Call clear()        │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ┌───────────────────────┬───┴────┘     └───────┐                                             └────────────────────────┘    └───────────────────────────┘
                                                    │                                                                                                                                                                                                                                                                                                                                                                                   └───────────────────────┘                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                       │                      │                                                          │                              │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                               │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │                       │                      │                                                          │                              │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                               │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │                       │                      │                                                          │                              │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                               │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │                       │                      │                                                          │                              │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                               │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │                       │                      │                                                          │                              │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                               │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │                       │                      ▼                                                          │                              ▼
                                                    ▼                                                                                                                                                                                                                                                                                                                                                                                               │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    │                       │                ┌────────────────────────┐                                       ▼              ┌───────────────────────────────┐
                                    ┌───────────────────────────────┐                                                                                                                                                                                                                                                                                                                                                                               ▼                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    ▼                       ▼                │ parser/*.spec.js files │                             ┌───────────────────┐    │ Generate CSS styles           │
                                    │ Remove trailing whitespace    │                                                                                                                                                                                                                                                                                                                                                                      ┌────────────────┐                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              ┌────────────────┐      ┌─────────────────────────┐    │ - flow-text.spec.js    │                             │ diag.db.getData() │    │ - .label, .cluster-label      │
                                    │ src.replace(/}\s*\n/g, '}\n') │                                                                                                                                                                                                                                                                                                                                                                      < FlowDB Methods >                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              │ flowDb.spec.ts │      │ flowChartShapes.spec.js │    │ - flow-edges.spec.js   │                             │ as LayoutData     │    │ - .node, .edgePath            │
                                    └───────────────────────────────┘                                                                                                                                                                                                                                                                                                                                                                      └────────────────┘                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              └────────────────┘      └─────────────────────────┘    │ - flow-style.spec.js   │                             └───────────────────┘    │ - .flowchart-link, .edgeLabel │
                                                    │                                                                                                                                                                                                                                                                                                                                                                                       ││││ ││││ ││││ │                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      │ - subgraph.spec.js     │                                       │              └───────────────────────────────┘
                                                    │                                                                                                                                                     ┌─────────────────────────────────────────────────────────────────┬─────────────────────────────────────────────────────┬─────────────────────────────┬───────────────────────────┬────────────────────────────┬──────────────────┴┴┴┴─┴┘└┼─┴┴┴┴─┴────────────────────┬───────────────────────────────┬────────────────────────────────────────────┬──────────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐                                                                                         └────────────────────────┘                                       │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          │                              │
                                                    │                                                                                                                                                     │                                                                 │                                                     │                             │                           │                            │                          │                           │                               │                                            │                                          │                                                                                                                                                   │                                                                                                                                                                                           │                                                                                                                                                          ▼                              │
                                                    ▼                                                                                                                                                     ▼                                                                 ▼                                                     ▼                             ▼                           ▼                            ▼                          ▼                           ▼                               ▼                                            ▼                                          ▼                                                                                                                                                   ▼                                                                                                                                                                                           ▼                                                                                                                                             ┌─────────────────────────┐                 ▼
                                    ┌───────────────────────────────┐                                                                                                 ┌─────────────────────────────────────┐                                  ┌─────────────────────────────────────┐                  ┌───────────────────────────────────────┐    ┌───────────────────┐    ┌─────────────────────────────────┐    ┌─────────────────────┐    ┌──────────────────────────┐    ┌──────────────────────────┐    ┌───────────────────────────────────────┐    ┌─────────────────────────────────────┐    ┌───────────┐                                                                                                                                   ┌───────────────────┐                                                                                                                                                                           ┌───────────────────────┐                                                                                                                     │ Setup layout data       │        ┌─────────────────┐
                                    │ parser/flow.jison             │                                                                                                 │ addVertex(id, textObj, type, style, │                                  │ addLink(_start[], _end[], linkData) │                  │ addSingleLink(_start, _end, type, id) │    │ setDirection(dir) │    │ addSubGraph(nodes[], id, title) │    │ addClass(id, style) │    │ setClass(ids, className) │    │ setTooltip(ids, tooltip) │    │ setClickEvent(id, functionName, args) │    │ setClickFun(id, functionName, args) │    │ getData() │                                                                                                                                   < Utility Functions >                                                                                                                                                                           │ commonDb.js functions │                                                                                                                     │ - type, layoutAlgorithm │        │ getIconStyles() │
                                    │ flowJisonParser.parse(newSrc) │                                                                                                 │ classes, dir, props, metadata)      │                                  └─────────────────────────────────────┘                  └───────────────────────────────────────┘    └───────────────────┘    └─────────────────────────────────┘    └─────────────────────┘    └──────────────────────────┘    └──────────────────────────┘    └───────────────────────────────────────┘    └─────────────────────────────────────┘    └───────────┘                                                                                                                                   └───────────────────┘                                                                                                                                                                           └───────────────────────┘                                                                                                                     │ - direction, spacing    │        └─────────────────┘
                                    └───────────────────────────────┘                                                                                                 └─────────────────────────────────────┘                                                                                                               │                                                                                                                                                                                                                                                                            │ │  │ │  │                                                                                                                                     │ │  │ │  │ │  │  │                                                                                                                                                                             │  │   │   │  │   │   │                                                                                                                      │ - markers, diagramId    │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                ┌─────────────────────────────┬──────────────────────────────────────────────────────────────────────────────────────────────────────┬──────────────────────┴─┴─┬┴─┘  └─────────────────────┐                                 ┌────────────────────────┬────────────────────────┬────────────────────────┬──┴─┴──┴─┘  └─┴──┴──┴───┬───────────────────────┬───────────────────────┬───────────────────────┐                               ┌────────────────────────┬────────────────────────┬───────────────┴──┴───┘   │  └───┴───┴───────────────┬─────────────────────────┬───────────────────────┐                                                    └─────────────────────────┘
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                                                     │                       │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              ┌──────────────────────┘                       └──────────────────────────────────────────────────┐
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    │                                                                                                                                    │                                                                                                                                  │                                                                                                                │                             │                                                                                                      │                          │                           │                                 │                        │                        │                        │                        │                       │                       │                       │                               │                        │                        │                          │                          │                         │                       │                              │                                                                                                 │
                                                    ▼                                                                                                                                    ▼                                                                                                                                  ▼                                                                                                                ▼                             ▼                                                                                                      ▼                          ▼                           ▼                                 ▼                        ▼                        ▼                        ▼                        ▼                       ▼                       ▼                       ▼                               ▼                        ▼                        ▼                          ▼                          ▼                         ▼                       ▼                              ▼                                                                                                 ▼
                                        ┌───────────────────────┐                                                                                                              ┌───────────────────┐                                                                                                               ┌─────────────────┐                                                                                  ┌───────────────────────────────┐    ┌────────────────────────────┐                                                                             ┌───────────────────┐      ┌─────────────────────┐    ┌──────────────────────────────┐    ┌─────────────────┐     ┌──────────────┐         ┌────────────────┐         ┌───────────────┐         ┌────────────┐         ┌────────────────┐           ┌─────────┐          ┌─────────────────┐        ┌───────────────┐         ┌───────────────┐      ┌─────────────────────┐     ┌─────────────────────┐      ┌───────────────────┐      ┌───────────────────┐         ┌─────────┐       ┌──────────────────────────┐                                                                             ┌──────────────────┐
                                        │ Parse Graph Structure │                                                                                                              < Vertex Processing >                                                                                                               < Edge Processing >                                                                                  │ Collect nodes[] from vertices │    │ Collect edges[] from edges │                                                                             │ Process subgraphs │      │ addNodeFromVertex() │    │ destructEdgeType()           │    │ lookUpDomId(id) │     │ getClasses() │         │ getDirection() │         │ getVertices() │         │ getEdges() │         │ getSubGraphs() │           │ clear() │          │ defaultConfig() │        │ setAccTitle() │         │ getAccTitle() │      │ setAccDescription() │     │ getAccDescription() │      │ setDiagramTitle() │      │ getDiagramTitle() │         │ clear() │       │ render(data4Layout, svg) │                                                                             < Layout Algorithm >
                                        └───────────────────────┘                                                                                                              └───────────────────┘                                                                                                               └─────────────────┘                                                                                  └───────────────────────────────┘    └────────────────────────────┘                                                                             │ - parentDB Map    │      │ for each vertex     │    │ arrowTypeStart, arrowTypeEnd │    └─────────────────┘     └──────────────┘         └────────────────┘         └───────────────┘         └────────────┘         └────────────────┘           └─────────┘          └─────────────────┘        └───────────────┘         └───────────────┘      └─────────────────────┘     └─────────────────────┘      └───────────────────┘      └───────────────────┘         └─────────┘       └──────────────────────────┘                                                                             └──────────────────┘
                                         │    │     │    │     │                                                                                                                │     │     │     │                                                                                                                 │   │   │   │   │                                                                                                                                                                                                                                   │ - subGraphDB Map  │      └─────────────────────┘    └──────────────────────────────┘                                                                                                                                                                                                                                                                                                                                                                                                    │                        │                                                                               │       │        │
           ┌────────────────────────┬────┴────┘     └────┴───┬─┴──────────────────────┬───────────────────────┐                                ┌───────────────────────────┬────┴─────┘     └─────┴───┬───────────────────────────┐                                     ┌─────────────────────────┬─────────────────┴───┘   │   └───┴────────────────────────────────────────────────────────────────────────────────────────────────────────────┬───────────────────────────┐                                                                                          └───────────────────┘                 │                                                                                                                                                                                                                                                                                                                                                                                   ┌───────────────────────────────────────────────────────────────┘                        └───────────────────────────────────────────────┐                             ┌─┘       └────────┴───┬─────────────────────────┐
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                │                           │                          │                           │                                     │                         │                         │                                                                                                                    │                           │                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        │                             │                      │                         │
           │                        │                        │                        │                       │                                ▼                           │                          ▼                           ▼                                     ▼                         ▼                         ▼                                                                                                                    ▼                           ▼                                                                                                                                │                                                                                                                                                                                                                                                                                                                                                                                   │                                                                                                                                        ▼                             ▼                      ▼                         ▼
           ▼                        ▼                        ▼                        ▼                       ▼               ┌──────────────────────────┐                 ▼                    ┌─────────────────────┐    ┌──────────────────────────────┐    ┌──────────────────────────┐    ┌───────────────────┐    ┌────────────────────────┐                                                                                     ┌────────────────────────────────┐    ┌─────────────────────┐                                                                                                          ▼                                                                                                                                                                                                                                                                                                                                                                                   ▼                                                                                                                                ┌────────────────────┐         ┌───────────┐          ┌───────────────┐       ┌────────────────────┐
┌────────────────┐          ┌─────────────┐         ┌─────────────────┐        ┌───────────────┐        ┌──────────────┐      │ Create FlowVertex object │    ┌────────────────────────────┐    │ Parse YAML metadata │    │ Set vertex properties        │    │ Create FlowEdge object   │    │ Process link text │    │ Set edge properties    │                                                                                     │ Generate edge ID               │    │ Validate edge limit │                                                                                                  ┌───────────────┐                                                                                                                                                                                                                                                                                                                                                      ┌───────────────────────────────────┐                                                                                                                 │ flowChartShapes.js │         │ dagre     │          │ dagre-wrapper │       │ elk                │
│ Parse Vertices │          │ Parse Edges │         │ Parse Subgraphs │        │ Parse Classes │        │ Parse Styles │      │ - id, labelType, domId   │    │ sanitizeText(textObj.text) │    │ yaml.load(yamlData) │    │ - shape, label, icon, form   │    │ - start, end, type, text │    │ - sanitizeText()  │    │ - type, stroke, length │                                                                                     │ getEdgeId(start, end, counter) │    │ maxEdges check      │                                                                                                  < Node Creation >                                                                                                                                                                                                                                                                                                                                                      │ setupViewPortForSVG(svg, padding) │                                                                                                                 │ Shape Functions    │         │ (default) │          │ (v2 renderer) │       │ (external package) │
└────────────────┘          └─────────────┘         └─────────────────┘        └───────────────┘        └──────────────┘      │ - styles[], classes[]    │    └────────────────────────────┘    └─────────────────────┘    │ - pos, img, constraint, w, h │    │ - labelType, classes[]   │    │ - strip quotes    │    └────────────────────────┘                                                                                     └────────────────────────────────┘    └─────────────────────┘                                                                                                  └───────────────┘                                                                                                                                                                                                                                                                                                                                                      └───────────────────────────────────┘                                                                                                                 └────────────────────┘         └───────────┘          └───────────────┘       └────────────────────┘
                                                                                                                              └──────────────────────────┘                                                                 └──────────────────────────────┘    └──────────────────────────┘    └───────────────────┘                                                                                                                                                                                                                                                                                   │   │    │    │                                                                                                                                                                                                                                                                                                                                                                         │                                                                                                                                              │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ┌───────────────────────────┬──┴───┘    └────┴──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┬───────────────────────────────────┐                                                                                                                                                          │                                                                                                                                              │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                           │                                                                                                                                                                                           │                                   │                                                                                                                                                          │                                                                                                                                              │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                           │                                                                                                                                                                                           │                                   │                                                                                                                                                          │                                                                                                                                              │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │                           ▼                                                                                                                                                                                           │                                   │                                                                                                                                                          │                                                                                                                                              │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │           ┌─────────────────────────┐                                                                                                                                                                                 │                                   │                                                                                                                                                          ▼                                                                                                                                              │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ▼           │ Create base node        │                                                                                                                                                                                 ▼                                   ▼                                                                                                                                            ┌──────────────────────────┐                                                                                                                                 ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  ┌────────────────────────────┐    │ - id, label, parentId   │                                                                                                                                                                                 ┌──────────────────────────────┐    ┌───────────────────────────┐                                                                                                                │ Process vertex links     │                                                                                                                        ┌─────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  │ findNode(nodes, vertex.id) │    │ - cssStyles, cssClasses │                                                                                                                                                                                 │ getCompiledStyles(classDefs) │    │ getTypeFromVertex(vertex) │                                                                                                                │ - create anchor elements │                                                                                                                        < Shape Functions >
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  └────────────────────────────┘    │ - shape, domId, tooltip │                                                                                                                                                                                 └──────────────────────────────┘    └───────────────────────────┘                                                                                                                │ - handle click events    │                                                                                                                        └─────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    └─────────────────────────┘                                                                                                                                                                                                                                                                                                                                                                  └──────────────────────────┘                                                                                                                         │ │ │ │  │ │ │  │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  │                        │                                      ┌────────────────────────────┬─────────────────────────────┬────────────────────────┴─┴─┘ └──┴┬┴─┴──┴───────────────────────────────┬────────────────────────────────────┬───────────────────────────────────────────────┬────────────────────────────────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  └┐                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   │                       │                                      │                            │                             │                                  │                                     │                                    │                                               │                                            │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   ▼                       ▼                                      ▼                            ▼                             ▼                                  ▼                                     ▼                                    ▼                                               ▼                                            ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         ┌────────────────┐       ┌──────────────────┐    ┌──────────────────────────────┐    ┌─────────────────────────────┐    ┌─────────────────────────────────────────┐    ┌────────────────────────────────┐    ┌───────────────────────────────┐    ┌──────────────────────────────────────────┐    ┌───────────────────────────────────────┐    ┌────────────────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         < Event Handling >       │ Final SVG Output │    │ question(parent, bbox, node) │    │ hexagon(parent, bbox, node) │    │ rect_left_inv_arrow(parent, bbox, node) │    │ lean_right(parent, bbox, node) │    │ lean_left(parent, bbox, node) │    │ insertPolygonShape(parent, w, h, points) │    │ intersectPolygon(node, points, point) │    │ intersectRect(node, point) │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         └────────────────┘       └──────────────────┘    └──────────────────────────────┘    └─────────────────────────────┘    └─────────────────────────────────────────┘    └────────────────────────────────┘    └───────────────────────────────┘    └──────────────────────────────────────────┘    └───────────────────────────────────────┘    └────────────────────────────┘
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          │      │       │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          ┌───────────────┘      └┐      └──────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          │                       │                         │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          │                       │                         │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          │                       │                         │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          ▼                       ▼                         ▼
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        ┌────────────────────────┐    ┌────────────────────────┐    ┌───────────────────────────────────┐
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        │ setupToolTips(element) │    │ bindFunctions(element) │    │ utils.runFunc(functionName, args) │
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        └────────────────────────┘    └────────────────────────┘    └───────────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![flowchart_code_flow svg](../tests/svg-snapshots/flowchart/flowchart_code_flow.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
---
references:
  - "File: /packages/mermaid/src/diagrams/flowchart/flowDiagram.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/flowDb.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/flowDetector.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/flowDetector-v2.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/flowRenderer-v3-unified.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/styles.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/types.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/flowChartShapes.js"
  - "File: /packages/mermaid/src/diagrams/flowchart/parser/flowParser.ts"
  - "File: /packages/mermaid/src/diagrams/flowchart/elk/detector.ts"
generationTime: 2025-07-23T10:31:53.266Z
---
flowchart TD
    %% Entry Points and Detection
    Input["User Input Text"] --> Detection{Detection Phase}
    
    Detection --> flowDetector["flowDetector.ts<br/>detector(txt, config)"]
    Detection --> flowDetectorV2["flowDetector-v2.ts<br/>detector(txt, config)"]
    Detection --> elkDetector["elk/detector.ts<br/>detector(txt, config)"]
    
    flowDetector --> |"Checks /^\s*graph/"| DetectLegacy{Legacy Flowchart?}
    flowDetectorV2 --> |"Checks /^\s*flowchart/"| DetectNew{New Flowchart?}
    elkDetector --> |"Checks /^\s*flowchart-elk/"| DetectElk{ELK Layout?}
    
    DetectLegacy --> |Yes| LoadDiagram
    DetectNew --> |Yes| LoadDiagram
    DetectElk --> |Yes| LoadDiagram
    
    %% Loading Phase
    LoadDiagram["loader() function"] --> flowDiagram["flowDiagram.ts<br/>diagram object"]
    
    flowDiagram --> DiagramStructure{Diagram Components}
    DiagramStructure --> Parser["parser: flowParser"]
    DiagramStructure --> Database["db: new FlowDB()"]
    DiagramStructure --> Renderer["renderer: flowRenderer-v3-unified"]
    DiagramStructure --> Styles["styles: flowStyles"]
    DiagramStructure --> Init["init: (cnf: MermaidConfig)"]
    
    %% Parser Phase
    Parser --> flowParser["parser/flowParser.ts<br/>newParser.parse(src)"]
    flowParser --> |"Preprocesses src"| RemoveWhitespace["Remove trailing whitespace<br/>src.replace(/}\s*\n/g, '}\n')"]
    RemoveWhitespace --> flowJison["parser/flow.jison<br/>flowJisonParser.parse(newSrc)"]
    
    flowJison --> ParseGraph["Parse Graph Structure"]
    ParseGraph --> ParseVertices["Parse Vertices"]
    ParseGraph --> ParseEdges["Parse Edges"]
    ParseGraph --> ParseSubgraphs["Parse Subgraphs"]
    ParseGraph --> ParseClasses["Parse Classes"]
    ParseGraph --> ParseStyles["Parse Styles"]
    
    %% Database Phase - FlowDB Class
    Database --> FlowDBClass["flowDb.ts<br/>FlowDB class"]
    
    FlowDBClass --> DBInit["constructor()<br/>- Initialize counters<br/>- Bind methods<br/>- Setup toolTips<br/>- Call clear()"]
    
    DBInit --> DBMethods{FlowDB Methods}
    
    DBMethods --> addVertex["addVertex(id, textObj, type, style,<br/>classes, dir, props, metadata)"]
    DBMethods --> addLink["addLink(_start[], _end[], linkData)"]
    DBMethods --> addSingleLink["addSingleLink(_start, _end, type, id)"]
    DBMethods --> setDirection["setDirection(dir)"]
    DBMethods --> addSubGraph["addSubGraph(nodes[], id, title)"]
    DBMethods --> addClass["addClass(id, style)"]
    DBMethods --> setClass["setClass(ids, className)"]
    DBMethods --> setTooltip["setTooltip(ids, tooltip)"]
    DBMethods --> setClickEvent["setClickEvent(id, functionName, args)"]
    DBMethods --> setClickFun["setClickFun(id, functionName, args)"]
    
    %% Vertex Processing
    addVertex --> VertexProcess{Vertex Processing}
    VertexProcess --> CreateVertex["Create FlowVertex object<br/>- id, labelType, domId<br/>- styles[], classes[]"]
    VertexProcess --> SanitizeText["sanitizeText(textObj.text)"]
    VertexProcess --> ParseMetadata["Parse YAML metadata<br/>yaml.load(yamlData)"]
    VertexProcess --> SetVertexProps["Set vertex properties<br/>- shape, label, icon, form<br/>- pos, img, constraint, w, h"]
    
    %% Edge Processing  
    addSingleLink --> EdgeProcess{Edge Processing}
    EdgeProcess --> CreateEdge["Create FlowEdge object<br/>- start, end, type, text<br/>- labelType, classes[]"]
    EdgeProcess --> ProcessLinkText["Process link text<br/>- sanitizeText()<br/>- strip quotes"]
    EdgeProcess --> SetEdgeProps["Set edge properties<br/>- type, stroke, length"]
    EdgeProcess --> GenerateEdgeId["Generate edge ID<br/>getEdgeId(start, end, counter)"]
    EdgeProcess --> ValidateEdgeLimit["Validate edge limit<br/>maxEdges check"]
    
    %% Data Collection
    DBMethods --> GetData["getData()"]
    GetData --> CollectNodes["Collect nodes[] from vertices"]
    GetData --> CollectEdges["Collect edges[] from edges"]
    GetData --> ProcessSubGraphs["Process subgraphs<br/>- parentDB Map<br/>- subGraphDB Map"]
    GetData --> AddNodeFromVertex["addNodeFromVertex()<br/>for each vertex"]
    GetData --> ProcessEdgeTypes["destructEdgeType()<br/>arrowTypeStart, arrowTypeEnd"]
    
    %% Node Creation
    AddNodeFromVertex --> NodeCreation{Node Creation}
    NodeCreation --> FindExistingNode["findNode(nodes, vertex.id)"]
    NodeCreation --> CreateBaseNode["Create base node<br/>- id, label, parentId<br/>- cssStyles, cssClasses<br/>- shape, domId, tooltip"]
    NodeCreation --> GetCompiledStyles["getCompiledStyles(classDefs)"]
    NodeCreation --> GetTypeFromVertex["getTypeFromVertex(vertex)"]
    
    %% Rendering Phase
    Renderer --> flowRendererV3["flowRenderer-v3-unified.ts<br/>draw(text, id, version, diag)"]
    
    flowRendererV3 --> RenderInit["Initialize rendering<br/>- getConfig()<br/>- handle securityLevel<br/>- getDiagramElement()"]
    
    RenderInit --> GetLayoutData["diag.db.getData()<br/>as LayoutData"]
    GetLayoutData --> SetupLayoutData["Setup layout data<br/>- type, layoutAlgorithm<br/>- direction, spacing<br/>- markers, diagramId"]
    
    SetupLayoutData --> CallRender["render(data4Layout, svg)"]
    CallRender --> SetupViewPort["setupViewPortForSVG(svg, padding)"]
    SetupViewPort --> ProcessLinks["Process vertex links<br/>- create anchor elements<br/>- handle click events"]
    
    %% Shape Rendering
    CallRender --> ShapeSystem["flowChartShapes.js<br/>Shape Functions"]
    
    ShapeSystem --> ShapeFunctions{Shape Functions}
    ShapeFunctions --> question["question(parent, bbox, node)"]
    ShapeFunctions --> hexagon["hexagon(parent, bbox, node)"]
    ShapeFunctions --> rect_left_inv_arrow["rect_left_inv_arrow(parent, bbox, node)"]
    ShapeFunctions --> lean_right["lean_right(parent, bbox, node)"]
    ShapeFunctions --> lean_left["lean_left(parent, bbox, node)"]
    
    ShapeFunctions --> insertPolygonShape["insertPolygonShape(parent, w, h, points)"]
    ShapeFunctions --> intersectPolygon["intersectPolygon(node, points, point)"]
    ShapeFunctions --> intersectRect["intersectRect(node, point)"]
    
    %% Styling System
    Styles --> stylesTS["styles.ts<br/>getStyles(options)"]
    stylesTS --> StyleOptions["FlowChartStyleOptions<br/>- arrowheadColor, border2<br/>- clusterBkg, mainBkg<br/>- fontFamily, textColor"]
    
    StyleOptions --> GenerateCSS["Generate CSS styles<br/>- .label, .cluster-label<br/>- .node, .edgePath<br/>- .flowchart-link, .edgeLabel"]
    GenerateCSS --> GetIconStyles["getIconStyles()"]
    
    %% Type System
    Parser --> TypeSystem["types.ts<br/>Type Definitions"]
    TypeSystem --> FlowVertex["FlowVertex interface"]
    TypeSystem --> FlowEdge["FlowEdge interface"]
    TypeSystem --> FlowClass["FlowClass interface"]
    TypeSystem --> FlowSubGraph["FlowSubGraph interface"]
    TypeSystem --> FlowVertexTypeParam["FlowVertexTypeParam<br/>Shape types"]
    
    %% Utility Functions
    DBMethods --> UtilityFunctions{Utility Functions}
    UtilityFunctions --> lookUpDomId["lookUpDomId(id)"]
    UtilityFunctions --> getClasses["getClasses()"]
    UtilityFunctions --> getDirection["getDirection()"]
    UtilityFunctions --> getVertices["getVertices()"]
    UtilityFunctions --> getEdges["getEdges()"]
    UtilityFunctions --> getSubGraphs["getSubGraphs()"]
    UtilityFunctions --> clear["clear()"]
    UtilityFunctions --> defaultConfig["defaultConfig()"]
    
    %% Event Handling
    ProcessLinks --> EventHandling{Event Handling}
    EventHandling --> setupToolTips["setupToolTips(element)"]
    EventHandling --> bindFunctions["bindFunctions(element)"]
    EventHandling --> runFunc["utils.runFunc(functionName, args)"]
    
    %% Common Database Functions
    DBMethods --> CommonDB["commonDb.js functions"]
    CommonDB --> setAccTitle["setAccTitle()"]
    CommonDB --> getAccTitle["getAccTitle()"]
    CommonDB --> setAccDescription["setAccDescription()"]
    CommonDB --> getAccDescription["getAccDescription()"]
    CommonDB --> setDiagramTitle["setDiagramTitle()"]
    CommonDB --> getDiagramTitle["getDiagramTitle()"]
    CommonDB --> commonClear["clear()"]
    
    %% Final Output
    ProcessLinks --> FinalSVG["Final SVG Output"]
    
    %% Layout Algorithm Selection
    SetupLayoutData --> LayoutAlgorithm{Layout Algorithm}
    LayoutAlgorithm --> Dagre["dagre<br/>(default)"]
    LayoutAlgorithm --> DagreWrapper["dagre-wrapper<br/>(v2 renderer)"]
    LayoutAlgorithm --> ELK["elk<br/>(external package)"]
    
    %% Testing Components
    FlowDBClass --> TestFiles["Test Files"]
    TestFiles --> flowDbSpec["flowDb.spec.ts"]
    TestFiles --> flowChartShapesSpec["flowChartShapes.spec.js"]
    TestFiles --> ParserTests["parser/*.spec.js files<br/>- flow-text.spec.js<br/>- flow-edges.spec.js<br/>- flow-style.spec.js<br/>- subgraph.spec.js"]
    
    %% Configuration
    Init --> ConfigSetup["Configuration Setup"]
    ConfigSetup --> FlowchartConfig["cnf.flowchart config"]
    ConfigSetup --> ArrowMarkers["arrowMarkerAbsolute"]
    ConfigSetup --> LayoutConfig["layout config"]
    ConfigSetup --> SetConfig["setConfig() calls"]
```

</details>

## git_workflow_td

`tests/fixtures/flowchart/git_workflow_td.mmd`

**Text**

```text
 ┌─────────────┐
 │ Working Dir │◄─┐
 └─────────────┘  │
  └─────┐         │
        │         │
        │         │
     git add      │
        │         │
        ▼         │
┌──────────────┐  │
│ Staging Area │  │
└──────────────┘  │
        │         │
        │         │
        │     git pull
   git commit     │
        │         │
        ▼         │
 ┌────────────┐   │
 │ Local Repo │   │
 └────────────┘   │
        │         │
        │         │
        │         │
    git push      │
  ┌─────┘         │
  ▼               │
 ┌─────────────┐  │
 │ Remote Repo │──┘
 └─────────────┘
```

<details>
<summary>SVG output</summary>

![git_workflow_td svg](../tests/svg-snapshots/flowchart/git_workflow_td.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    %% A typical git workflow
    Working[Working Dir] -->|git add| Staging[Staging Area]
    Staging -->|git commit| Local[Local Repo]
    Local -->|git push| Remote[Remote Repo]
    Remote -->|git pull| Working

```

</details>

## git_workflow

`tests/fixtures/flowchart/git_workflow.mmd`

**Text**

```text
┌─────────────┐┐                             ┌──────────────┐                               ┌────────────┐                      ┌───────►┌─────────────┐
│ Working Dir │└───────────git add──────────►│ Staging Area │──────────git commit──────────►│ Local Repo │───────────git push───┘        │ Remote Repo │
└─────────────┘◄──────┐                      └──────────────┘                               └────────────┘                              ┌└─────────────┘
                      └──────────────────────┐                                                                           ┌──────────────┘
                                             │                                                                           │
                                             └──────────────────────────git pull─────────────────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![git_workflow svg](../tests/svg-snapshots/flowchart/git_workflow.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    %% A typical git workflow
    Working[Working Dir] -->|git add| Staging[Staging Area]
    Staging -->|git commit| Local[Local Repo]
    Local -->|git push| Remote[Remote Repo]
    Remote -->|git pull| Working

```

</details>

## hexagon_flow

`tests/fixtures/flowchart/hexagon_flow.mmd`

**Text**

```text
        ┌───────┐
        │ Input │
        └───────┘
            │
            │
            ▼
       ┌─────────┐
       < Process >
       └─────────┘
        │       │
       ┌┘       └┐
       ▼         ▼
┌────────┐     ┌─────┐
│ Output │     │ Log │
└────────┘     └─────┘
```

<details>
<summary>SVG output</summary>

![hexagon_flow svg](../tests/svg-snapshots/flowchart/hexagon_flow.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A{{Process}} --> B[Output]
    C[Input] --> A
    A --> D[Log]

```

</details>

## http_request

`tests/fixtures/flowchart/http_request.mmd`

**Text**

```text
                         ┌────────┐
                         │ Client │◄────────────┐
                         └────────┘             │
                          └───┐                 │
                              │                 │
                              │                 │
                              │                 │
                        HTTP Request            │
                              │                 │
                              │                 │
                              │                 │
                              ▼                 │
                         ┌────────┐             │
                         │ Server │             │
                         └────────┘             │
                              │                 │
                              │                 │
                              │           HTTP Response
                              │                 │
                              ▼                 │
                     ┌────────────────┐         │
                     < Authenticated? >         │
                     └────────────────┘         │
         ┌────────────┘          ┌───┘          │
         │                       │              │
        Yes                     No              │
         │                       │              │
         ▼                       ▼              │
┌─────────────────┐       ┌──────────────────┐  │
│ Process Request │       │ 401 Unauthorized │  │
└─────────────────┘       └──────────────────┘  │
             │                  │               │
             └─────────┐        └────┐          │
                       │             │          │
                       │             │          │
                       ▼             ▼          │
                      ┌───────────────┐         │
                      │ Send Response │─────────┘
                      └───────────────┘
```

<details>
<summary>SVG output</summary>

![http_request svg](../tests/svg-snapshots/flowchart/http_request.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    Client[Client] -->|HTTP Request| Server[Server]
    Server --> Auth{Authenticated?}
    Auth -->|Yes| Process[Process Request]
    Auth -->|No| Reject[401 Unauthorized]
    Process --> Response[Send Response]
    Reject --> Response
    Response -->|HTTP Response| Client

```

</details>

## inline_edge_labels

`tests/fixtures/flowchart/inline_edge_labels.mmd`

**Text**

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     │
     │
    yes
     │
     ▼
 ┌──────┐
 │ Next │
 └──────┘
     ┆
     ┆
     ┆
   retry
     ┆
     ▼
 ┌───────┐
 │ Again │
 └───────┘
     ┃
     ┃
final step
     ┃
     ┃
     ▼
 ┌──────┐
 │ Done │
 └──────┘
     │
     │
     │
    no
     │
     ▼
 ┌──────┐
 │ Stop │
 └──────┘
```

<details>
<summary>SVG output</summary>

![inline_edge_labels svg](../tests/svg-snapshots/flowchart/inline_edge_labels.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] -- yes --> B[Next]
    B -. retry .-> C[Again]
    C == "final step" ==> D[Done]
    D -- no --> E[Stop]

```

</details>

## inline_label_flowchart

`tests/fixtures/flowchart/inline_label_flowchart.mmd`

**Text**

```text
                       ╭───────╮
                       │ Start │
                       ╰───────╯
                           │
                           │
                           │
                           │
                           ▼
                  ┌────────────────┐
                  │ Ingest Request │
                  └────────────────┘
                   │              │
                  ┌┘              └────────┐
                  │                        │
                  │                        │
                  ▼                        ▼
        ┌───────────────┐                ┌───────────┐
        │ Parse Payload │                │ Audit Log │
        └───────────────┘                └───────────┘
         │       │                             │
         └┐      │                             │
          │      └─────────────┐               │
          │                    │               │
          ▼                    │               │
┌──────────────┐               │               │
│ Lookup Cache │               │               │
└──────────────┘               │               │
 └──────┐     │                │               │
        │     │                │               │
        │     │                │               │
        │     │                │               │
        │     │                │               │
       hit    └──miss          │               │
        │          │           │               │
        │          └──────┐    │               │
        │                 │    │               │
        │                 │    │               │
        ▼                 ▼    ▼               │
┌──────────────┐         ┌────────┐            │
│ Serve Cached │         < Valid? >            │
└──────────────┘         └────────┘            │
        │              ┌──┘   ┌──┘             │
        │              │      │                │
        │              │      │                │
        │              │      │                │
        │              │      │                │
        │             yes    no                └─────────────┐
        │              │      │                              │
        │              └──────┼────────────────┐             │
        │                     │                │             │
        │                     ▼                ▼             │
        │                ┌────────┐           ┌────────────┐ │
        │                │ Reject │───────────< Route Type >─┼──────────────────────────────────┐
        │                └────────┘           └────────────┘ │                                  │
        │                 ┆            ┌───────┘          └──┤                                  │
        │                 └┄┄┄┐        │       ┌─────────────┤                                  │
        │                     ┆        │       │             │                                  │
        │                     ┆        │       │             │                                  │
        │                     ┆        │       │             │                                  │
        │                     ▼        │       │             │                                  │
        │              ┌─────────────┐ │       │           sync                                 │
        │              │ Notify User │ │       │             │                                  │
        │              └─────────────┘ │       │             │                                  │
        │                              └───────┼───async─────┼──────────────────┐               │
        │                                      │             │                  │               │
        │                                      │             │                  │               │
        │                                      │             │                  │               │
        │                                      │             ▼                  ▼               │
        │                                      │     ┌───────────────┐         ┌─────────────┐  │
        │                                      │     │ Sync Pipeline │         │ Enqueue Job │◄━┫
        │                                      │     └───────────────┘         └─────────────┘  ┃
        │                                      │             │                  │               ┃
        │                     ┌────────────────┼─────────────┼──────────────────┘               ┃
        │                     │                │             │                                  ┃
        │                     │                │             │                                  ┃
        │                     ▼                │             │                                  ┃
        │       ┌─────────────┐                │             │                                  ┃
        │       │ Worker Pool │                │             │                                  ┃
        │       └─────────────┘                │             │                                  ┃
        │              │                       │             │                                  ┃
        │              │                       │             │                                  ┃
        │              │                       │             │                                  ┃
        │              │                       │             │                                  ┃
        │              ▼                       │             │                                  ┃
        └─────┐ ┌─────────────┐                │             │                                  ┃
              │ │ Process Job │                │             │                                  ┃
              │ └─────────────┘                │             │                                  ┃
              │  │     ┌─────┘                 │             │                                  ┃
         ┌────┼──┘     │                       │             │                                  ┃
         │    │        │                       │             │                                  ┃
         │    │        │                       │             │                                  ┃
         │    │        │                       │             │                                  ┃
         ▼    │        │                       │             │                                  ┃
 ┌──────────┐ │      warn                      │             │                                  ┃
 < Success? > │        │                       │             │                                  ┃
 └──────────┘ │        │                       │             │                                  ┃
  └──┐ ┌───┘  │        │                       │             │                                  ┃
     │ │┌─────┘        │                       │             │                                  ┃
     │ ││              │                       │             │                                  ┃
     │ ││              │                       │             │                                  ┃
     │ ││              │                       │             │                                  ┃
     │ ││              │                       │             │                                  ┃
    yes││              ▼                       │             │                                  ┃
     │no│      ┌──────────────┐                └────┐        │                                  ┃
     │ ││      │ Page On-call │                     │        │                                  ┃
     │ ││      └──────────────┘                     │        │                                  ┃
     │ ││                    └┄┄┄┐                  │        │                                  ┃
     └─┴┼──────┬─────────────────┼─────────┐        │        │                                  ┃
        │      │                 ┆         │        │        │                                  ┃
        │      │         ┌───────┼─────────┼────────┼────────┘                                  ┃
        │      │         │       ┆         │        │                                           ┃
        │      │         │       ┆         │        │                                           ┃
        │      ▼         ▼       ┆         ▼        │                                           ┃
        │     ┌────────────────┐ ┆        ┌───────┐ │                                           ┃
        │     │ Persist Result │ ┆        │ Retry │━╋━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┫
        │     └────────────────┘ ┆        └───────┘ │                                           │
 ┌─────┬┴┄┄┄┄┄┄┄┄┄┄┄┼┄┄┄┄┄┄┄┄┄┄┄┄┘                  │                                           │
 │  ┌──┼────────────┘                               │                                           │
 │  │  ┆      ┌─────────────────────────────────────┘                                           │
 │  │  ┆      │                                                                                 │
 ▼  ▼  ▼      ▼                                                                                 │
┌──────────────┐                                                                                │
│ Emit Metrics │◄───────────────────────────────────────────────────────────────────────────────┘
└──────────────┘
        │
        │
        │
        │
        ▼
    ╭──────╮
    │ Done │
    ╰──────╯
```

<details>
<summary>SVG output</summary>

![inline_label_flowchart svg](../tests/svg-snapshots/flowchart/inline_label_flowchart.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
flowchart TD
  start((Start)) --> ingest[Ingest Request]
  ingest --> parse[Parse Payload]
  parse --> validate{Valid?}

  validate -- no --> reject[Reject]
  reject -.-> notify[Notify User]
  reject --> metrics[Emit Metrics]

  validate -- yes --> route{Route Type}
  route -- sync --> sync[Sync Pipeline]
  route -- async --> queue[Enqueue Job]

  queue --> worker[Worker Pool]
  worker --> process[Process Job]
  process --> success{Success?}

  success -- no --> retry[Retry]
  retry ==> queue

  success -- yes --> persist[Persist Result]
  sync --> persist
  persist --> metrics

  parse --> cache[Lookup Cache]
  cache -- hit --> fastpath[Serve Cached]
  fastpath --> metrics
  cache -- miss --> validate

  ingest --> audit[Audit Log]
  audit --> metrics

  process -- warn --> alert[Page On-call]
  alert -.-> metrics

  metrics --> End((Done))

```

</details>

## label_spacing

`tests/fixtures/flowchart/label_spacing.mmd`

**Text**

```text
        ┌───┐
        │ A │
        └───┘
  ┌──────┘ └──────┐
  │               │
  │               │
  │               │
valid          invalid
  │               │
  │               │
  │               │
  ▼               ▼
┌───┐           ┌───┐
│ B │           │ C │
└───┘           └───┘
```

<details>
<summary>SVG output</summary>

![label_spacing svg](../tests/svg-snapshots/flowchart/label_spacing.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    %% Test case for edge label spacing with branching edges
    %% Labels should not overlap when multiple edges branch from the same source
    A -->|valid| B
    A -->|invalid| C

```

</details>

## labeled_edges

`tests/fixtures/flowchart/labeled_edges.mmd`

**Text**

```text
    ┌───────┐
    │ Begin │
    └───────┘
        │
        │
        │
        │
   initialize
        │
        │
        │
        ▼
    ┌───────┐
    │ Setup │◄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┐
    └───────┘                     retry
     └──┐                           ┆
        │                           ┆
    configure                       ┆
        │                           ┆
        ▼                           ┆
   ┌────────┐                       ┆
   < Valid? >                       ┆
   └────────┘                       ┆
    └┐     └──┐                     ┆
     │        │                     ┆
     │        │                     ┆
     │        │                     ┆
    yes      no                     ┆
     │        │                     ┆
     │        └────┐                ┆
     │             │                ┆
     ▼             ▼                ┆
┌─────────┐       ┌──────────────┐  ┆
│ Execute │       │ Handle Error │┄┄┘
└─────────┘       └──────────────┘
```

<details>
<summary>SVG output</summary>

![labeled_edges svg](../tests/svg-snapshots/flowchart/labeled_edges.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    Start[Begin] -->|initialize| Setup[Setup]
    Setup -->|configure| Config{Valid?}
    Config -->|yes| Run[Execute]
    Config -->|no| Error[Handle Error]
    Error -.->|retry| Setup

```

</details>

## left_right

`tests/fixtures/flowchart/left_right.mmd`

**Text**

```text
┌────────────┐      ┌──────────────┐     ┌────────────────┐
│ User Input │─────►│ Process Data │────►│ Display Result │
└────────────┘      └──────────────┘     └────────────────┘
```

<details>
<summary>SVG output</summary>

![left_right svg](../tests/svg-snapshots/flowchart/left_right.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    Input[User Input] --> Process[Process Data]
    Process --> Output[Display Result]

```

</details>

## mixed_shape_chain

`tests/fixtures/flowchart/mixed_shape_chain.mmd`

**Text**

```text
  ┌───────┐
  │ Start │
  └───────┘
      │
      │
      ▼
┌──────────┐
< Decision >
└──────────┘
      │
      │
      ▼
 ┌─────────┐
 < Hexagon >
 └─────────┘
      │
      │
      ▼
   ┌─────┐
   │ End │
   └─────┘
```

<details>
<summary>SVG output</summary>

![mixed_shape_chain svg](../tests/svg-snapshots/flowchart/mixed_shape_chain.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B{Decision}
    B --> C{{Hexagon}}
    C --> D[End]

```

</details>

## multi_edge_labeled

`tests/fixtures/flowchart/multi_edge_labeled.mmd`

**Text**

```text
  ┌───┐
  │ A │
  └───┘
   │ └──┐
   │    │
   │    │
   │ path 2
path 1  │
   │    │
   │ ┌──┘
   │ │
   ▼ ▼
  ┌───┐
  │ B │
  └───┘
    │
    │
    │
    │
    ▼
  ┌───┐
  │ C │
  └───┘
```

<details>
<summary>SVG output</summary>

![multi_edge_labeled svg](../tests/svg-snapshots/flowchart/multi_edge_labeled.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A -->|path 1| B
    A -->|path 2| B
    B --> C

```

</details>

## multi_edge

`tests/fixtures/flowchart/multi_edge.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
 │ │
 │ │
 ▼ ▼
┌───┐
│ B │
└───┘
```

<details>
<summary>SVG output</summary>

![multi_edge svg](../tests/svg-snapshots/flowchart/multi_edge.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A --> B
    A --> B

```

</details>

## multi_subgraph_direction_override

`tests/fixtures/flowchart/multi_subgraph_direction_override.mmd`

**Text**

```text
┌────────── a ──────────┐
│ ┌───┐  ┌───┐    ┌───┐ │
│ │ b │  │ u │    │ c │ │
│ └───┘  └───┘    └───┘ │
│ ┌┘       │         │  │
└─┼────────┼─────────┼──┘
  │        │         │
  │        │         │
  │        │         │
  │        │         │
  │        │         │
  │        │         │
  ▼        │         ▼
┌───┐      │       ┌───┐
│ b │      │       │ c │
└───┘      └┐      └───┘
 │ │        │        │
 └┐└──────┐ │       ┌┘
  ▼       ▼ ▼       ▼
┌───┐    ┌───┐    ┌───┐
│ d │    │ f │    │ e │
└───┘    └───┘    └───┘
          │ │      │
          │ └────┐ │
          │      │ │
          └┐     │ │
           │     │ │
           │     │ │
       ┌───┼─ g ─┼─┼──┐
       │   ▼     ▼ ▼  │
       │ ┌───┐  ┌───┐ │
       │ │ b │  │ a │ │
       │ └───┘  └───┘ │
       └──────────────┘
```

<details>
<summary>SVG output</summary>

![multi_subgraph_direction_override svg](../tests/svg-snapshots/flowchart/multi_subgraph_direction_override.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
flowchart TB

%% --- Top container (A) ---
subgraph A["a"]
  direction LR
  Ab["b"]
  Au["u"]
  Ac["c"]
end
 
%% --- Middle (outside containers) ---
Bmid["b"]
D["d"]
F["f"]

Cmid["c"]
E["e"]

%% --- Bottom container (G) ---
subgraph G["g"]
  direction LR
  Gb["b"]
  Ga["a"]
end

%% --- Edges (match the figure) ---
Ab --> Bmid
Bmid --> D
Bmid --> F
Au --> F

Ac --> Cmid
Cmid --> E

F --> Gb
F --> Ga
E --> Ga

%% --- Light styling to resemble container shading (optional) ---
style A fill:#e9efff,stroke:#1f4fff,stroke-width:1px
style G fill:#e9efff,stroke:#1f4fff,stroke-width:1px
classDef node fill:#f7f9ff,stroke:#1f4fff,stroke-width:1px,color:#000
class Ab,Au,Ac,Bmid,Cmid,D,E,F,Gb,Ga node
```

</details>

## multi_subgraph

`tests/fixtures/flowchart/multi_subgraph.mmd`

**Text**

```text
┌───────────── Frontend ─────────────┐             ┌───────────── Backend ─────────────┐
│        ┌────┐       ┌─────┐        │             │       ┌────────┐     ┌────┐       │
│        │ UI │──────►│ API │────────┼─────────────┼──────►│ Server │────►│ DB │       │
│        └────┘       └─────┘        │             │       └────────┘     └────┘       │
└────────────────────────────────────┘             └───────────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![multi_subgraph svg](../tests/svg-snapshots/flowchart/multi_subgraph.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
subgraph sg1[Frontend]
A[UI] --> B[API]
end
subgraph sg2[Backend]
C[Server] --> D[DB]
end
B --> C

```

</details>

## multiple_cycles

`tests/fixtures/flowchart/multiple_cycles.mmd`

**Text**

```text
  ┌─────┐
  │ Top │◄┐
  └─────┘ │
   │      │
   └─┐    │
     ▼▲   │
┌────────┐│
│ Middle ││
└────────┘│
     │   ││
 ┌───┘   ││
 ▼       ││
┌────────┐│
│ Bottom ││
└────────┘
```

<details>
<summary>SVG output</summary>

![multiple_cycles svg](../tests/svg-snapshots/flowchart/multiple_cycles.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Top] --> B[Middle]
    B --> C[Bottom]
    C --> A
    C --> B

```

</details>

## narrow_fan_in

`tests/fixtures/flowchart/narrow_fan_in.mmd`

**Text**

```text
┌───┐    ┌───┐    ┌───┐
│ A │    │ B │    │ C │
└───┘    └───┘    └───┘
    │      │      │
    └─────┐│┌─────┘
          ▼▼▼
         ┌───┐
         │ X │
         └───┘
```

<details>
<summary>SVG output</summary>

![narrow_fan_in svg](../tests/svg-snapshots/flowchart/narrow_fan_in.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[A] --> D[X]
    B[B] --> D
    C[C] --> D

```

</details>

## nested_subgraph_edge

`tests/fixtures/flowchart/nested_subgraph_edge.mmd`

**Text**

```text
                 ┌────────┐
                 │ Client │
                 └────────┘
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      ▼
┌────────────────── Cloud ──────────────────┐
│    ┌──────────── US East ────────────┐    │
│    │                                 │    │
│    │   ┌─────────┐     ┌─────────┐   │    │
│    │   │ Server1 │     │ Server2 │   │    │
│    │   └─────────┘     └─────────┘   │    │
│    │                                 │    │
│    └─────────────────────────────────┘    │
└───────────────────────────────────────────┘
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      ▼
               ┌────────────┐
               │ Monitoring │
               └────────────┘
```

<details>
<summary>SVG output</summary>

![nested_subgraph_edge svg](../tests/svg-snapshots/flowchart/nested_subgraph_edge.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph cloud[Cloud]
        subgraph region[US East]
            Server1
            Server2
        end
    end
    Client --> cloud
    cloud --> Monitoring

```

</details>

## nested_subgraph_only

`tests/fixtures/flowchart/nested_subgraph_only.mmd`

**Text**

```text
┌───── Outer ─────┐
│  ┌── Inner ──┐  │
│  │   ┌───┐   │  │
│  │   │ A │   │  │
│  │   └───┘   │  │
│  │     │     │  │
│  │     │     │  │
│  │     ▼     │  │
│  │   ┌───┐   │  │
│  │   │ B │   │  │
│  │   └───┘   │  │
│  └───────────┘  │
└─────────────────┘
```

<details>
<summary>SVG output</summary>

![nested_subgraph_only svg](../tests/svg-snapshots/flowchart/nested_subgraph_only.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
subgraph outer[Outer]
subgraph inner[Inner]
A --> B
end
end

```

</details>

## nested_subgraph

`tests/fixtures/flowchart/nested_subgraph.mmd`

**Text**

```text
┌───────── Outer ─────────┐
│        ┌───────┐        │
│        │ Start │        │
│        └───────┘        │
│            │            │
│            │            │
│            │            │
│            │            │
│            │            │
│            │            │
│            │            │
│    ┌──── Inner ────┐    │
│    │       ▼       │    │
│    │  ┌─────────┐  │    │
│    │  │ Process │  │    │
│    │  └─────────┘  │    │
│    │       │       │    │
│    │       │       │    │
│    │       ▼       │    │
│    │    ┌─────┐    │    │
│    │    │ End │    │    │
│    │    └─────┘    │    │
│    └───────────────┘    │
└─────────────────────────┘
```

<details>
<summary>SVG output</summary>

![nested_subgraph svg](../tests/svg-snapshots/flowchart/nested_subgraph.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
subgraph outer[Outer]
A[Start]
subgraph inner[Inner]
B[Process] --> C[End]
end
end
A --> B

```

</details>

## nested_with_siblings

`tests/fixtures/flowchart/nested_with_siblings.mmd`

**Text**

```text
┌────────────────────────────────── Outer ──────────────────────────────────┐
│       ┌───────── Left ─────────┐         ┌──────── Right ─────────┐       │
│       │     ┌───┐    ┌───┐     │         │     ┌───┐    ┌───┐     │       │
│       │     │ A │───►│ B │─────┼─────────┼────►│ C │───►│ D │     │       │
│       │     └───┘    └───┘     │         │     └───┘    └───┘     │       │
│       └────────────────────────┘         └────────────────────────┘       │
└───────────────────────────────────────────────────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![nested_with_siblings svg](../tests/svg-snapshots/flowchart/nested_with_siblings.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
subgraph outer[Outer]
subgraph left[Left]
A --> B
end
subgraph right[Right]
C --> D
end
end
B --> C

```

</details>

## right_left

`tests/fixtures/flowchart/right_left.mmd`

**Text**

```text
┌───────┐     ┌─────────┐    ┌────────┐
│ Begin │◄────│ Process │◄───│ Finish │
└───────┘     └─────────┘    └────────┘
```

<details>
<summary>SVG output</summary>

![right_left svg](../tests/svg-snapshots/flowchart/right_left.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph RL
    End[Finish] --> Middle[Process]
    Middle --> Start[Begin]

```

</details>

## self_loop_labeled

`tests/fixtures/flowchart/self_loop_labeled.mmd`

**Text**

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     │
     │
     │
     ▼
┌────────┐───┐
< Retry? > retry
└────────┘◄──┘
     │
     │
     │
     │
   done
     │
     │
     │
     ▼
  ┌─────┐
  │ End │
  └─────┘
```

<details>
<summary>SVG output</summary>

![self_loop_labeled svg](../tests/svg-snapshots/flowchart/self_loop_labeled.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B{Retry?}
    B -->|retry| B
    B -->|done| C[End]

```

</details>

## self_loop_with_others

`tests/fixtures/flowchart/self_loop_with_others.mmd`

**Text**

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     │
     ▼
┌─────────┐───┐
│ Process │   │
└─────────┘◄──┘
     │
     │
     ▼
  ┌─────┐
  │ End │
  └─────┘
```

<details>
<summary>SVG output</summary>

![self_loop_with_others svg](../tests/svg-snapshots/flowchart/self_loop_with_others.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B[Process]
    B --> B
    B --> C[End]

```

</details>

## self_loop

`tests/fixtures/flowchart/self_loop.mmd`

**Text**

```text
┌─────────┐───┐
│ Process │   │
└─────────┘◄──┘
```

<details>
<summary>SVG output</summary>

![self_loop svg](../tests/svg-snapshots/flowchart/self_loop.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Process] --> A

```

</details>

## shapes_basic

`tests/fixtures/flowchart/shapes_basic.mmd`

**Text**

```text
 ┌───────────┐
 │ Rectangle │
 └───────────┘
       │
       │
       ▼
  ╭─────────╮
  │ Rounded │
  ╰─────────╯
       │
       │
       ▼
  ╭─────────╮
  │ Stadium │
  ╰─────────╯
       │
       │
       ▼
┌────────────┐
║ Subroutine ║
└────────────┘
       │
       │
       ▼
 ┌──────────┐
 ( Cylinder )
 └──────────┘
       │
       │
       ▼
 ┌──────────┐
 < Decision >
 └──────────┘
       │
       │
       ▼
  ┌─────────┐
  < Hexagon >
  └─────────┘
```

<details>
<summary>SVG output</summary>

![shapes_basic svg](../tests/svg-snapshots/flowchart/shapes_basic.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    rect[Rectangle]
    round(Rounded)
    stadium([Stadium])
    sub[[Subroutine]]
    cyl[(Cylinder)]
    diamond{Decision}
    hex{{Hexagon}}
    rect --> round --> stadium --> sub --> cyl --> diamond --> hex

```

</details>

## shapes_degenerate

`tests/fixtures/flowchart/shapes_degenerate.mmd`

**Text**

```text
┌───────┐
│ Cloud │
└───────┘
    │
    │
    ▼
┌──────┐
│ Bolt │
└──────┘
    │
    │
    ▼
┌──────┐
│ Bang │
└──────┘
    │
    │
    ▼
┌──────┐
│ Icon │
└──────┘
    │
    │
    ▼
┌──────┐
│ Hour │
└──────┘
    │
    │
    ▼
 ┌─────┐
 │ Tri │
 └─────┘
    │
    │
    ▼
┌──────┐
│ Flip │
└──────┘
    │
    │
    ▼
┌───────┐
│ Notch │
└───────┘
```

<details>
<summary>SVG output</summary>

![shapes_degenerate svg](../tests/svg-snapshots/flowchart/shapes_degenerate.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    cloud@{shape: cloud, label: "Cloud"}
    bolt@{shape: bolt, label: "Bolt"}
    bang@{shape: bang, label: "Bang"}
    icon@{shape: icon, label: "Icon"}
    hourglass@{shape: hourglass, label: "Hour"}
    tri@{shape: tri, label: "Tri"}
    flip@{shape: flip-tri, label: "Flip"}
    notch@{shape: notch-pent, label: "Notch"}
    cloud --> bolt --> bang --> icon --> hourglass --> tri --> flip --> notch

```

</details>

## shapes_document

`tests/fixtures/flowchart/shapes_document.mmd`

**Text**

```text
  ┌─────┐
  │ Doc │
  └~~~~~┘
     │
     │
     ▼
 ┌──────┐
 │ Docs ││
 └~~~~~~┘│
  ───│───┘
     │
     ▼
┌───────╱┐
│ TagDoc │
└~~~~~~~~┘
     │
     │
     ▼
 ┌─────╱┐
 │ Card │
 └──────┘
     │
     │
     ▼
  ┌────╱┐
  │ Tag │
  └─────┘
```

<details>
<summary>SVG output</summary>

![shapes_document svg](../tests/svg-snapshots/flowchart/shapes_document.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    doc@{shape: doc, label: "Doc"}
    docs@{shape: docs, label: "Docs"}
    tagdoc@{shape: tag-doc, label: "TagDoc"}
    card@{shape: card, label: "Card"}
    tag@{shape: tag-rect, label: "Tag"}
    doc --> docs --> tagdoc --> card --> tag

```

</details>

## shapes_junction

`tests/fixtures/flowchart/shapes_junction.mmd`

**Text**

```text
●  ───► ◉  ───► ⊗
```

<details>
<summary>SVG output</summary>

![shapes_junction svg](../tests/svg-snapshots/flowchart/shapes_junction.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    j1@{shape: sm-circ}
    j2@{shape: fr-circ}
    j3@{shape: cross-circ}
    j1 --> j2 --> j3

```

</details>

## shapes_special

`tests/fixtures/flowchart/shapes_special.mmd`

**Text**

```text
┃
┃
┃ ─────►  Note
┃
```

<details>
<summary>SVG output</summary>

![shapes_special svg](../tests/svg-snapshots/flowchart/shapes_special.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph LR
    fork@{shape: fork}
    note@{shape: text, label: "Note"}
    fork --> note

```

</details>

## shapes

`tests/fixtures/flowchart/shapes.mmd`

**Text**

```text
┌────────────────┐
│ Rectangle Node │
└────────────────┘
         │
         │
         ▼
 ╭──────────────╮
 │ Rounded Node │
 ╰──────────────╯
         │
         │
         ▼
 ┌──────────────┐
 < Diamond Node >
 └──────────────┘
```

<details>
<summary>SVG output</summary>

![shapes svg](../tests/svg-snapshots/flowchart/shapes.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    rect[Rectangle Node]
    round(Rounded Node)
    diamond{Diamond Node}
    rect --> round --> diamond

```

</details>

## simple_cycle

`tests/fixtures/flowchart/simple_cycle.mmd`

**Text**

```text
 ┌───────┐
 │ Start │◄──┐
 └───────┘   │
  │          │
  └──┐       │
     ▼       │
┌─────────┐  │
│ Process │  │
└─────────┘  │
     │       │
   ┌─┘       │
   ▼         │
  ┌─────┐    │
  │ End │────┘
  └─────┘
```

<details>
<summary>SVG output</summary>

![simple_cycle svg](../tests/svg-snapshots/flowchart/simple_cycle.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B[Process]
    B --> C[End]
    C --> A

```

</details>

## simple_subgraph

`tests/fixtures/flowchart/simple_subgraph.mmd`

**Text**

```text
┌── Process ───┐
│   ┌───────┐  │
│   │ Start │  │
│   └───────┘  │
│       │      │
│       │      │
│       ▼      │
│  ┌────────┐  │
│  │ Middle │  │
│  └────────┘  │
│       │      │
└───────┼──────┘
        │
        │
        │
        │
        │
        │
        ▼
     ┌─────┐
     │ End │
     └─────┘
```

<details>
<summary>SVG output</summary>

![simple_subgraph svg](../tests/svg-snapshots/flowchart/simple_subgraph.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
subgraph sg1[Process]
A[Start] --> B[Middle]
end
B --> C[End]

```

</details>

## simple

`tests/fixtures/flowchart/simple.mmd`

**Text**

```text
┌───────┐
│ Start │
└───────┘
    │
    │
    ▼
 ┌─────┐
 │ End │
 └─────┘
```

<details>
<summary>SVG output</summary>

![simple svg](../tests/svg-snapshots/flowchart/simple.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B[End]

```

</details>

## skip_edge_collision

`tests/fixtures/flowchart/skip_edge_collision.mmd`

**Text**

```text
      ┌───────┐
      │ Start │
      └───────┘
       │  │
      ┌┘  │
      ▼   │
┌────────┐│
│ Step 1 ││
└────────┘│
     │    │
     │    └───┐
     ▼        │
┌────────┐    │
│ Step 2 │┌───┘
└────────┘│
      │   │
      └─┐ │
        ▼ ▼
       ┌─────┐
       │ End │
       └─────┘
```

<details>
<summary>SVG output</summary>

![skip_edge_collision svg](../tests/svg-snapshots/flowchart/skip_edge_collision.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Start] --> B[Step 1]
    B --> C[Step 2]
    C --> D[End]
    A --> D

```

</details>

## stacked_fan_in

`tests/fixtures/flowchart/stacked_fan_in.mmd`

**Text**

```text
   ┌─────┐
   │ Top │
   └─────┘
    │   └─┐
    │     │
    ▼     │
┌─────┐   │
│ Mid │   │
└─────┘ ┌─┘
    │   │
    │   │
    ▼   ▼
   ┌─────┐
   │ Bot │
   └─────┘
```

<details>
<summary>SVG output</summary>

![stacked_fan_in svg](../tests/svg-snapshots/flowchart/stacked_fan_in.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[Top] --> B[Mid]
    B --> C[Bot]
    A --> C

```

</details>

## style-basic

`tests/fixtures/flowchart/style-basic.mmd`

**Text**

```text
┌───────┐
│ Alpha │
└───────┘
```

<details>
<summary>SVG output</summary>

![style-basic svg](../tests/svg-snapshots/flowchart/style-basic.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
A[Alpha]
style A fill:#ffeeaa,stroke:#333,color:#111

```

</details>

## style-precedence

`tests/fixtures/flowchart/style-precedence.mmd`

**Text**

```text
┌───────┐
│ Alpha │
└───────┘
```

<details>
<summary>SVG output</summary>

![style-precedence svg](../tests/svg-snapshots/flowchart/style-precedence.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
style A fill:#ffeeaa,stroke:#333
A[Alpha]
style A stroke:#555,color:#111

```

</details>

## subgraph_as_node_edge

`tests/fixtures/flowchart/subgraph_as_node_edge.mmd`

**Text**

```text
     ┌────────┐
     │ Client │
     └────────┘
          │
          │
          │
          │
          │
          │
          ▼
┌──── Backend ─────┐
│                  │
│  ┌────────────┐  │
│  │ API Server │  │
│  └────────────┘  │
│         │        │
│         │        │
│         ▼        │
│   ┌──────────┐   │
│   │ Database │   │
│   └──────────┘   │
│                  │
└──────────────────┘
          │
          │
          │
          │
          │
          │
          ▼
      ┌──────┐
      │ Logs │
      └──────┘
```

<details>
<summary>SVG output</summary>

![subgraph_as_node_edge svg](../tests/svg-snapshots/flowchart/subgraph_as_node_edge.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph sg1[Backend]
        API[API Server]
        DB[Database]
        API --> DB
    end
    Client --> sg1
    sg1 --> Logs

```

</details>

## subgraph_direction_cross_boundary

`tests/fixtures/flowchart/subgraph_direction_cross_boundary.mmd`

**Text**

```text
      ┌───┐
      │ C │
      └───┘
       │ │
       └┐│
        ▼│
      ┌───┐
      │ E │
      └───┘
        ││
       ┌┘│
       │ │
       │ │
       │ │
       │ │
       │ │
┌─ Horizontal Section ─┐
│      ▼ ▼             │
│     ┌───┐  ┌───┐     │
│     │ A │─►│ B │     │
│     └───┘  └───┘     │
│       ┌─────┘ │      │
└───────┼───────┼──────┘
        │       │
        │       │
        │     ┌─┘
        │     │
        │     │
        │     │
        ▼     │
      ┌───┐   │
      │ F │   │
      └───┘ ┌─┘
         │  │
         └┐ │
          ▼ ▼
         ┌───┐
         │ D │
         └───┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_cross_boundary svg](../tests/svg-snapshots/flowchart/subgraph_direction_cross_boundary.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph sg1[Horizontal Section]
        direction LR
        A --> B
    end
    C --> E
    E --> A
    C --> A
    B --> F
    F --> D
    B --> D

```

</details>

## subgraph_direction_isolated

`tests/fixtures/flowchart/subgraph_direction_isolated.mmd`

**Text**

```text
┌───── Horizontal ──────┐
│ ┌───┐  ┌───┐    ┌───┐ │   ┌───┐
│ │ A │─►│ B │───►│ C │ │   │ D │
│ └───┘  └───┘    └───┘ │   └───┘
└──────────────┼────────┴────┘
               │
               │
               │
               │
               │
               │
               │
               ▼
             ┌───┐
             │ E │
             └───┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_isolated svg](../tests/svg-snapshots/flowchart/subgraph_direction_isolated.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph sg1[Horizontal]
        direction LR
        A --> B --> C
    end
    D --> E

```

</details>

## subgraph_direction_lr

`tests/fixtures/flowchart/subgraph_direction_lr.mmd`

**Text**

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     └─┐
       │
       │
       │
       │
       │
┌──────┼── Horizontal Flow ──────────┐
│      ▼                             │
│ ┌────────┐  ┌────────┐  ┌────────┐ │
│ │ Step 1 │─►│ Step 2 │─►│ Step 3 │ │
│ └────────┘  └────────┘  └────────┘ │
│                     ┌────┘         │
└─────────────────────┼──────────────┘
                      │
                      │
                      │
                      │
                      │
                      │
                      ▼
                 ┌─────┐
                 │ End │
                 └─────┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_lr svg](../tests/svg-snapshots/flowchart/subgraph_direction_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    Start --> A
    subgraph sg1[Horizontal Flow]
        direction LR
        A[Step 1] --> B[Step 2] --> C[Step 3]
    end
    C --> End

```

</details>

## subgraph_direction_mixed

`tests/fixtures/flowchart/subgraph_direction_mixed.mmd`

**Text**

```text
  ┌─ Left to Right ─┐
  │  ┌───┐  ┌───┐   │
  │  │ A │─►│ B │   │
  │  └───┘  └───┘   │
  │           │     │
  └───────┼───┴─────┘
          │
          │
          │
          │
          │
          │
          │
          │
┌─ Bottom to Top ─┐
│         │       │
│      ┌───┐      │
│      │ D │      │
│      └───┘      │
│        ▲│       │
│       ┌┘▼       │
│      ┌───┐      │
│      │ C │      │
│      └───┘      │
└─────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_mixed svg](../tests/svg-snapshots/flowchart/subgraph_direction_mixed.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph lr_group[Left to Right]
        direction LR
        A --> B
    end
    subgraph bt_group[Bottom to Top]
        direction BT
        C --> D
    end
    B --> C

```

</details>

## subgraph_direction_nested_both

`tests/fixtures/flowchart/subgraph_direction_nested_both.mmd`

**Text**

```text
       ┌───┐
       │ D │
       └───┘
  ┌─────┘
  │
  │
  │
  │
  │
  │
  │
  │
  │
┌─┼──── Outer LR ───────┐
│ │       ┌─ Inner BT ─┐│
│ │       │            ││
│ └┐      │   ┌───┐    ││
│  │      │   │ B │    ││
│  │      │   └───┘    ││
│  │      │     ▲      ││
│  ▼      │     │      ││
│ ┌───┐   │   ┌───┐    ││
│ │ C │───┼──►│ A │    ││
│ └───┘   │   └───┘    ││
│         └────────────┘│
└───────────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_nested_both svg](../tests/svg-snapshots/flowchart/subgraph_direction_nested_both.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph outer[Outer LR]
        direction LR
        subgraph inner[Inner BT]
            direction BT
            A --> B
        end
        C --> A
    end
    D --> C

```

</details>

## subgraph_direction_nested_mixed

`tests/fixtures/flowchart/subgraph_direction_nested_mixed.mmd`

**Text**

```text
          ┌───┐
          │ E │
          └───┘
      ┌────┘
      │
      │
      │
      │
      │
      │
      │
      │
      │
┌─────┼─────── Outer LR ──────────────┐
│     │                 ┌─ Inner BT ─┐│
│     └────────────┐    │   ┌───┐    ││
│                  │    │   │ B │    ││
│                  ▼    │   └───┘    ││
│                 ┌───┐ │     ▲   ┌───┐
│                 │ C │─┼─────┼──►│ D││
│                 └───┘ │   ┌───┐ └───┘
│                       │   │ A │    ││
│                       │   └───┘    ││
│                       └────────────┘│
└─────────────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_nested_mixed svg](../tests/svg-snapshots/flowchart/subgraph_direction_nested_mixed.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph outer[Outer LR]
        direction LR
        subgraph inner[Inner BT]
            direction BT
            A --> B
        end
        C --> D
    end
    E --> C

```

</details>

## subgraph_direction_nested

`tests/fixtures/flowchart/subgraph_direction_nested.mmd`

**Text**

```text
┌──── Vertical Outer ─────┐
│  ┌───┐                  │
│  │ D │                  │
│  └───┘                  │
│    │                    │
│    │                    │
│    │                    │
│    │                    │
│    │                    │
│    │                    │
│    │                    │
│┌── Horizontal Inner ───┐│
││   ▼                   ││
││ ┌───┐  ┌───┐    ┌───┐ ││
││ │ A │─►│ B │───►│ C │ ││
││ └───┘  └───┘    └───┘ ││
│└───────────────────────┘│
└─────────────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_direction_nested svg](../tests/svg-snapshots/flowchart/subgraph_direction_nested.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph outer[Vertical Outer]
        subgraph inner[Horizontal Inner]
            direction LR
            A --> B --> C
        end
        D --> A
    end

```

</details>

## subgraph_edges_bottom_top

`tests/fixtures/flowchart/subgraph_edges_bottom_top.mmd`

**Text**

```text
┌───────── Output ──────────┐
│   ┌────────┐    ┌─────┐   │
│   │ Result │    │ Log │   │
│   └────────┘    └─────┘   │
│        ▲           ▲      │
└────────┼───────────┼──────┘
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
  ┌──────┼── Input ──┼───────┐
  │      │           │       │
  │  ┌──────┐    ┌────────┐  │
  │  │ Data │    │ Config │  │
  │  └──────┘    └────────┘  │
  └──────────────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_edges_bottom_top svg](../tests/svg-snapshots/flowchart/subgraph_edges_bottom_top.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph BT
subgraph sg1[Input]
A[Data]
B[Config]
end
subgraph sg2[Output]
C[Result]
D[Log]
end
A --> C
B --> D

```

</details>

## subgraph_edges

`tests/fixtures/flowchart/subgraph_edges.mmd`

**Text**

```text
  ┌───────── Input ──────────┐
  │  ┌──────┐    ┌────────┐  │
  │  │ Data │    │ Config │  │
  │  └──────┘    └────────┘  │
  │      │           │       │
  └──────┼───────────┼───────┘
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
         │           │
┌────────┼ Output ───┼──────┐
│        ▼           ▼      │
│   ┌────────┐    ┌─────┐   │
│   │ Result │    │ Log │   │
│   └────────┘    └─────┘   │
└───────────────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_edges svg](../tests/svg-snapshots/flowchart/subgraph_edges.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
subgraph sg1[Input]
A[Data]
B[Config]
end
subgraph sg2[Output]
C[Result]
D[Log]
end
A --> C
B --> D

```

</details>

## subgraph_multi_word_title

`tests/fixtures/flowchart/subgraph_multi_word_title.mmd`

**Text**

```text
      ┌────────┐
      │ Source │
      └────────┘
           │
           │
           │
           │
           │
           │
           │
┌─ Data Processing Pipeline ─┐
│          ▼                 │
│     ┌─────────┐            │
│     │ Extract │            │
│     └─────────┘            │
│          │                 │
│          │                 │
│          ▼                 │
│    ┌───────────┐           │
│    │ Transform │           │
│    └───────────┘           │
│          │                 │
│          │                 │
│          ▼                 │
│      ┌──────┐              │
│      │ Load │              │
│      └──────┘              │
│          │                 │
└──────────┼─────────────────┘
           │
           │
           │
           │
           │
           │
           ▼
       ┌──────┐
       │ Sink │
       └──────┘
```

<details>
<summary>SVG output</summary>

![subgraph_multi_word_title svg](../tests/svg-snapshots/flowchart/subgraph_multi_word_title.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph "Data Processing Pipeline"
        Extract[Extract] --> Transform[Transform] --> Load[Load]
    end
    Source --> Extract
    Load --> Sink

```

</details>

## subgraph_numeric_id

`tests/fixtures/flowchart/subgraph_numeric_id.mmd`

**Text**

```text
┌─ Phase 1 ─┐
│    ┌───┐  │
│    │ A │  │
│    └───┘  │
│      │    │
│      │    │
│      ▼    │
│    ┌───┐  │
│    │ B │  │
│    └───┘  │
│      │    │
└──────┼────┘
       │
       │
       │
       │
       │
       │
       │
       │
       │
       │
┌─ Phase 2 ─┐
│      ▼    │
│    ┌───┐  │
│    │ C │  │
│    └───┘  │
│      │    │
│      │    │
│      ▼    │
│    ┌───┐  │
│    │ D │  │
│    └───┘  │
└───────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_numeric_id svg](../tests/svg-snapshots/flowchart/subgraph_numeric_id.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph 1phase[Phase 1]
        A --> B
    end
    subgraph 2phase[Phase 2]
        C --> D
    end
    B --> C

```

</details>

## subgraph_to_subgraph_edge

`tests/fixtures/flowchart/subgraph_to_subgraph_edge.mmd`

**Text**

```text
┌─────── Frontend ───────┐
│   ┌────────────────┐   │
│   │ User Interface │   │
│   └────────────────┘   │
│            │           │
│            │           │
│            ▼           │
│    ┌───────────────┐   │
│    │ State Manager │   │
│    └───────────────┘   │
│                        │
└────────────────────────┘
             │
             │
             │
             │
             │
             │
             │
             │
             │
             ▼
 ┌────── Backend ───────┐
 │                      │
 │    ┌────────────┐    │
 │    │ API Server │    │
 │    └────────────┘    │
 │           │          │
 │           │          │
 │           ▼          │
 │     ┌──────────┐     │
 │     │ Database │     │
 │     └──────────┘     │
 └──────────────────────┘
```

<details>
<summary>SVG output</summary>

![subgraph_to_subgraph_edge svg](../tests/svg-snapshots/flowchart/subgraph_to_subgraph_edge.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    subgraph frontend[Frontend]
        UI[User Interface]
        State[State Manager]
        UI --> State
    end
    subgraph backend[Backend]
        API[API Server]
        DB[Database]
        API --> DB
    end
    frontend --> backend

```

</details>

## very_narrow_fan_in

`tests/fixtures/flowchart/very_narrow_fan_in.mmd`

**Text**

```text
┌───┐    ┌───┐    ┌───┐    ┌───┐
│ X │    │ X │    │ X │    │ X │
└───┘    └───┘    └───┘    └───┘
    │       │      │       │
    └───────┴──┐┌┬─┴───────┘
               ▼▼▼
              ┌───┐
              │ Y │
              └───┘
```

<details>
<summary>SVG output</summary>

![very_narrow_fan_in svg](../tests/svg-snapshots/flowchart/very_narrow_fan_in.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
graph TD
    A[X] --> E[Y]
    B[X] --> E
    C[X] --> E
    D[X] --> E

```

</details>

# Class

## all_relations

`tests/fixtures/class/all_relations.mmd`

**Text**

```text
 ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐
 │ A │    │ C │    │ E │    │ G │    │ I │    │ K │    │ M │
 └───┘    └───┘    └───┘    └───┘    └───┘    └───┘    └───┘
   │        │        △        ◆        ◇        ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        │        │        │        │    directed dep ┆
   │    directed     │   composition   │   dependency    ┆
association │   inheritance   │   aggregation   ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        │        │        │        │        ┆        ┆
   │        ▼        │        │        │        ┆        ▼
 ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐
 │ B │    │ D │    │ F │    │ H │    │ J │    │ L │    │ N │
 └───┘    └───┘    └───┘    └───┘    └───┘    └───┘    └───┘
```

<details>
<summary>SVG output</summary>

![all_relations svg](../tests/svg-snapshots/class/all_relations.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
    A -- B : association
    C --> D : directed
    E <|-- F : inheritance
    G *-- H : composition
    I o-- J : aggregation
    K .. L : dependency
    M ..> N : directed dep

```

</details>

## animal_hierarchy

`tests/fixtures/class/animal_hierarchy.mmd`

**Text**

```text
                         ┌────────────────┐
                         │     Animal     │
                         ├────────────────┤
                         │    +int age    │
                         │ +String gender │
                         ├────────────────┤
                         │ +isMammal()    │
                         │ +mate()        │
                         └────────────────┘
                          △      △       △
                  ┌───────┘      └┐      └────────┐
                  │               │               │
┌───────────────────┐             │               │
│       Duck        │    ┌─────────────────┐    ┌───────────────┐
├───────────────────┤    │      Fish       │    │     Zebra     │
│ +String beakColor │    ├─────────────────┤    ├───────────────┤
├───────────────────┤    │ -int sizeInFeet │    │ +bool is_wild │
│ +swim()           │    ├─────────────────┤    ├───────────────┤
│ +quack()          │    │ -canEat()       │    │ +run()        │
└───────────────────┘    └─────────────────┘    └───────────────┘
```

<details>
<summary>SVG output</summary>

![animal_hierarchy svg](../tests/svg-snapshots/class/animal_hierarchy.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
    Animal <|-- Duck
    Animal <|-- Fish
    Animal <|-- Zebra
    Animal : +int age
    Animal : +String gender
    Animal: +isMammal()
    Animal: +mate()
    class Duck{
      +String beakColor
      +swim()
      +quack()
    }
    class Fish{
      -int sizeInFeet
      -canEat()
    }
    class Zebra{
      +bool is_wild
      +run()
    }

```

</details>

## cardinality_labels

`tests/fixtures/class/cardinality_labels.mmd`

**Text**

```text
 ┌──────┐     ┌───────┐
 │ User │     │ Order │
 └──────┘     └───────┘
     │            │
     │            │
     │   contains │
   owns           │
     │            │
     ▼            ▼
┌─────────┐    ┌──────┐
│ Session │    │ Item │
└─────────┘    └──────┘
```

<details>
<summary>SVG output</summary>

![cardinality_labels svg](../tests/svg-snapshots/class/cardinality_labels.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
%% Parity: endpoint cardinalities with compact labels.
classDiagram
User "1" --> "*" Session:owns
Order "0..1" --> "*" Item:contains

```

</details>

## class_labels

`tests/fixtures/class/class_labels.mmd`

**Text**

```text
┌──────┐
│ User │
└──────┘
    │
    │
    │
  reads
    │
    ▼
┌──────┐
│ Repo │
└──────┘
```

<details>
<summary>SVG output</summary>

![class_labels svg](../tests/svg-snapshots/class/class_labels.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
%% Parity: class display-label syntax acceptance.
classDiagram
class User["Application User"]
class Repo["Code Repository"]
User --> Repo:reads

```

</details>

## direction_bt

`tests/fixtures/class/direction_bt.mmd`

**Text**

```text
┌───┐
│ C │
└───┘
  ▲
  │
  │
┌───┐
│ B │
└───┘
  ▲
  │
  │
┌───┐
│ A │
└───┘
```

<details>
<summary>SVG output</summary>

![direction_bt svg](../tests/svg-snapshots/class/direction_bt.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
direction BT
A --> B
B --> C

```

</details>

## direction_lr

`tests/fixtures/class/direction_lr.mmd`

**Text**

```text
┌───┐    ┌───┐    ┌───┐
│ A │───►│ B │───►│ C │
└───┘    └───┘    └───┘
```

<details>
<summary>SVG output</summary>

![direction_lr svg](../tests/svg-snapshots/class/direction_lr.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
direction LR
A --> B
B --> C

```

</details>

## direction_rl

`tests/fixtures/class/direction_rl.mmd`

**Text**

```text
┌───┐    ┌───┐    ┌───┐
│ C │◄───│ B │◄───│ A │
└───┘    └───┘    └───┘
```

<details>
<summary>SVG output</summary>

![direction_rl svg](../tests/svg-snapshots/class/direction_rl.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
direction RL
A --> B
B --> C

```

</details>

## direction_tb

`tests/fixtures/class/direction_tb.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
  │
  │
  ▼
┌───┐
│ B │
└───┘
  │
  │
  ▼
┌───┐
│ C │
└───┘
```

<details>
<summary>SVG output</summary>

![direction_tb svg](../tests/svg-snapshots/class/direction_tb.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
direction TB
A --> B
B --> C

```

</details>

## inheritance_chain

`tests/fixtures/class/inheritance_chain.mmd`

**Text**

```text
         ┌─────────┐
         │ Vehicle │
         └─────────┘
          △       △
         ┌┘       └┐
         │         │
    ┌─────┐      ┌───────┐
    │ Car │      │ Truck │
    └─────┘      └───────┘
       △
       │
       │
┌─────────────┐
│ ElectricCar │
└─────────────┘
```

<details>
<summary>SVG output</summary>

![inheritance_chain svg](../tests/svg-snapshots/class/inheritance_chain.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
    class Vehicle
    class Car
    class Truck
    class ElectricCar
    Vehicle <|-- Car
    Vehicle <|-- Truck
    Car <|-- ElectricCar

```

</details>

## interface_realization

`tests/fixtures/class/interface_realization.mmd`

**Text**

```text
          ┌───────────────┐
          │ <<interface>> │
          │    Logger     │
          ├───────────────┤
          ├───────────────┤
          │ +log(message) │
          └───────────────┘
           △             △
          ┌┘             └┐
          ┆               ┆
          ┆               ┆
┌───────────────┐    ┌────────────┐
│ ConsoleLogger │    │ FileLogger │
└───────────────┘    └────────────┘
```

<details>
<summary>SVG output</summary>

![interface_realization svg](../tests/svg-snapshots/class/interface_realization.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
class Logger {
  <<interface>>
  +log(message)
}
class ConsoleLogger
class FileLogger
Logger <|.. ConsoleLogger
Logger <|.. FileLogger

```

</details>

## lollipop_interfaces

`tests/fixtures/class/lollipop_interfaces.mmd`

**Text**

```text
                ┌────────┐
InterfaceB      │ Client │
                └────────┘
     o               │
     │               │
     │               o
┌─────────┐
│ Service │      InterfaceA
└─────────┘
     │
     │
     o

InterfaceA
```

<details>
<summary>SVG output</summary>

![lollipop_interfaces svg](../tests/svg-snapshots/class/lollipop_interfaces.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
%% Parity: lollipop relations in both directions should keep all classes visible.
classDiagram
Service --() InterfaceA
InterfaceB ()-- Service
Client --() InterfaceA

```

</details>

## members

`tests/fixtures/class/members.mmd`

**Text**

```text
┌───────────────┐
│     User      │
├───────────────┤
│ +String name  │
│ +String email │
├───────────────┤
│ +login()      │
│ +logout()     │
└───────────────┘
        │
        │
        │
        │
     creates
        │
        │
        │
        ▼
┌───────────────┐
│    Session    │
├───────────────┤
│ +String token │
├───────────────┤
│ +isValid()    │
└───────────────┘
```

<details>
<summary>SVG output</summary>

![members svg](../tests/svg-snapshots/class/members.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
    class User {
        +String name
        +String email
        +login()
        +logout()
    }
    class Session {
        +String token
        +isValid()
    }
    User --> Session : creates

```

</details>

## namespaces

`tests/fixtures/class/namespaces.mmd`

**Text**

```text
      ┌───── Tools ─────┐
      │   ┌─────────┐   │
      │   │ Painter │   │
      │   └─────────┘   │
      │        │        │
      └────────┼────────┘
               │
               │
               │
               │
               │
               │
               │
               │
               │
               │
┌──────── BaseShapes ─────────┐
│              ▼              │
│        ┌──────────┐         │
│        │ Triangle │         │
│        └──────────┘         │
│              │              │
│              │              │
│              │              │
│              │              │
│              │              │
│              │              │
│              │              │
│    ┌─── Primitives ────┐    │
│    │         ▼         │    │
│    │   ┌───────────┐   │    │
│    │   │ Rectangle │   │    │
│    │   └───────────┘   │    │
│    └───────────────────┘    │
└─────────────────────────────┘
```

<details>
<summary>SVG output</summary>

![namespaces svg](../tests/svg-snapshots/class/namespaces.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
namespace BaseShapes {
  class Triangle
  namespace Primitives {
    class Rectangle
  }
}
namespace Tools {
  class Painter
}

Triangle --> Rectangle
Painter --> Triangle

```

</details>

## relationships

`tests/fixtures/class/relationships.mmd`

**Text**

```text
 ┌─────────┐
 │ Service │
 └─────────┘
      ┆
      ┆
authenticates
      ┆
      ┆
      ▼
  ┌──────┐
  │ User │
  └──────┘
      │
      │
      │
   places
      │
      ▼
  ┌───────┐
  │ Order │
  └───────┘
      ◆
      │
contains
      │
      │
      │
 ┌─────────┐
 │ Product │
 └─────────┘
```

<details>
<summary>SVG output</summary>

![relationships svg](../tests/svg-snapshots/class/relationships.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
    class User
    class Order
    class Product
    class Service
    User --> Order : places
    Order *-- Product : contains
    Service ..> User : authenticates

```

</details>

## simple

`tests/fixtures/class/simple.mmd`

**Text**

```text
┌────────┐
│ Animal │
└────────┘
     △
     │
     │
  ┌─────┐
  │ Dog │
  └─────┘
```

<details>
<summary>SVG output</summary>

![simple svg](../tests/svg-snapshots/class/simple.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
classDiagram
    class Animal
    class Dog
    Animal <|-- Dog

```

</details>

## two_way_relations

`tests/fixtures/class/two_way_relations.mmd`

**Text**

```text
┌───┐
│ A │
└───┘
  △
  │
  ▽
┌───┐
│ B │
└───┘
  ◇
  │
  ◇
┌───┐
│ C │
└───┘
```

<details>
<summary>SVG output</summary>

![two_way_relations svg](../tests/svg-snapshots/class/two_way_relations.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
%% Parity: two-way marker operator parsing.
classDiagram
A <|--|> B
B o--o C

```

</details>

## user_lollipop_repro

`tests/fixtures/class/user_lollipop_repro.mmd`

**Text**

```text
               ┌─────────┐
      foo      │ Class02 │
               └─────────┘
       o            │
       │            │
       │            │
       │            │
       │            │
┌────────────┐      │
│  Class01   │      o
├────────────┤
│ int amount │      bar
├────────────┤
│ draw()     │
└────────────┘
       │
       │
       │
       │
       o

      bar
```

<details>
<summary>SVG output</summary>

![user_lollipop_repro svg](../tests/svg-snapshots/class/user_lollipop_repro.svg)

</details>

<details>
<summary>Mermaid source</summary>

```
%% User repro from research 0043: lollipop lines must not drop implicit classes.
classDiagram
class Class01 {
  int amount
  draw()
}
Class01 --() bar
Class02 --() bar
foo ()-- Class01

```

</details>

