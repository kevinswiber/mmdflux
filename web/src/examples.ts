export interface PlaygroundExample {
  id: string;
  name: string;
  description: string;
  category: "flowchart" | "class";
  featured: boolean;
  input: string;
}

export const PLAYGROUND_EXAMPLES: PlaygroundExample[] = [
  {
    id: "flowchart-basics",
    name: "Flowchart Basics",
    description: "Linear flow with decision branch",
    category: "flowchart",
    featured: true,
    input: `graph TD
    A[Start] --> B{Ready?}
    B -->|Yes| C[Deploy]
    B -->|No| D[Wait]
    D --> B`,
  },
  {
    id: "flowchart-fanout",
    name: "Fan-out",
    description: "One source branching to multiple targets",
    category: "flowchart",
    featured: true,
    input: `graph LR
    API --> Auth
    API --> Billing
    API --> Search
    API --> Profile`,
  },
  {
    id: "flowchart-cycle",
    name: "Simple Cycle",
    description: "Looping flow with feedback edge",
    category: "flowchart",
    featured: false,
    input: `graph TD
    A[Start] --> B[Process]
    B --> C[Review]
    C --> A`,
  },
  {
    id: "flowchart-edge-styles",
    name: "Edge Styles",
    description: "Solid, dotted, thick, and open edges",
    category: "flowchart",
    featured: false,
    input: `graph TD
    A[Solid] --> B[Normal]
    C[Dotted] -.-> D[Arrow]
    E[Thick] ==> F[Arrow]
    G[Open] --- H[Line]`,
  },
  {
    id: "flowchart-labeled-edges",
    name: "Labeled Edges",
    description: "Labels and retry routing in one graph",
    category: "flowchart",
    featured: false,
    input: `graph TD
    Start[Begin] -->|initialize| Setup[Setup]
    Setup -->|configure| Config{Valid?}
    Config -->|yes| Run[Execute]
    Config -->|no| Error[Handle Error]
    Error -.->|retry| Setup`,
  },
  {
    id: "flowchart-direction-bt",
    name: "Bottom to Top",
    description: "Direction override using BT layout",
    category: "flowchart",
    featured: false,
    input: `graph BT
    Foundation[Foundation] --> Structure[Structure]
    Structure --> Roof[Roof]`,
  },
  {
    id: "flowchart-ci-pipeline",
    name: "CI Pipeline",
    description: "Deployment workflow with branch outcomes",
    category: "flowchart",
    featured: false,
    input: `graph LR
    Push[Git Push] --> Build[Build]
    Build --> Test[Run Tests]
    Test --> Lint[Lint Check]
    Lint --> Deploy{Deploy?}
    Deploy -->|staging| Staging[Staging Env]
    Deploy -->|production| Prod[Production]`,
  },
  {
    id: "flowchart-complex",
    name: "Complex Workflow",
    description: "Mixed edges, fan-in, and loopback",
    category: "flowchart",
    featured: false,
    input: `graph TD
    A[Input] --> B{Validate}
    B -->|valid| C[Process]
    B -->|invalid| D(Error Handler)
    C --> E{More Data?}
    E -->|yes| A
    E -->|no| F[Output]
    D -.-> G[Log Error]
    D ==> H[Notify Admin]
    G & H --> I[Cleanup]
    I --> F`,
  },
  {
    id: "flowchart-nested-subgraph",
    name: "Nested Subgraph",
    description: "Nested containers with shared edges",
    category: "flowchart",
    featured: false,
    input: `graph TD
    subgraph outer[Outer]
        A[Start]
        subgraph inner[Inner]
            B[Process] --> C[End]
        end
    end
    A --> B`,
  },
  {
    id: "flowchart-subgraph-direction-override",
    name: "Subgraph Direction Overrides",
    description: "Fixture-based mixed LR/BT subgraph directions",
    category: "flowchart",
    featured: true,
    input: `graph TD
    subgraph lr_group[Left to Right]
        direction LR
        A --> B
    end
    subgraph bt_group[Bottom to Top]
        direction BT
        C --> D
    end
    B --> C`,
  },
  {
    id: "class-basics",
    name: "Class Basics",
    description: "Simple class relationship",
    category: "class",
    featured: true,
    input: `classDiagram
    class Animal {
      +String name
      +eat()
    }
    class Dog {
      +bark()
    }
    Animal <|-- Dog`,
  },
  {
    id: "class-relationships",
    name: "Class Relationships",
    description: "Association, composition, and dependency",
    category: "class",
    featured: true,
    input: `classDiagram
    class User
    class Order
    class Product
    class Service
    User --> Order : places
    Order *-- Product : contains
    Service ..> User : authenticates`,
  },
];

export const DEFAULT_EXAMPLE_ID = "flowchart-basics";

export function findExampleById(id: string): PlaygroundExample | null {
  return PLAYGROUND_EXAMPLES.find((example) => example.id === id) ?? null;
}
