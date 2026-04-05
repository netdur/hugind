# System Architecture Specification: Python Microservice Template

## 1. Overview
This specification defines a modular, production-ready structure for a Python-based microservice. It follows a decoupled approach where business logic is isolated from external dependencies (API, Database, Message Brokers).

## 2. Directory Structure

```text
project-root/
├── .github/                # CI/CD workflows
├── cmd/                    # Application entry points
│   └── server/             # Main service entry point
│       └── main.py
├── src/                    # Core source code
│   ├── api/                # Transport layer (REST/gRPC/GraphQL)
│   │   ├── v1/             # API versioning
│   │   └── dependencies.py # Framework-specific DI (e.g., FastAPI deps)
│   ├── core/               # Shared kernel/cross-cutting concerns
│   │   ├── config.py       # Pydantic settings/env vars
│   │   ├── exceptions.py   # Custom domain exceptions
│   │   └── security.py     # Auth/JWT logic
│   ├── domain/             # Pure business logic (The 'Heart')
│   │   ├── models/         # Pure Data Classes / Domain Models
│   │   ├── services/       # Business use-cases/orchestration
│   │   └── repository.py   # Interface definitions (Abstract Base Classes)
│   ├── infrastructure/     # External implementations (Adapters)
│   │   ├── database/       # SQLAlchemy/Tortoise/Mongo implementations
│   │   ├── clients/        # Third-party API clients (Stripe, AWS, etc.)
│   │   └── repositories/   # Concrete implementations of domain repositories
│   └── main.py             # App factory/orchestrator
├── tests/                  # Test suite
│   ├── unit/               # Isolated domain logic tests
│   ├── integration/        # Repository/DB tests
│   └── e2e/                # API endpoint tests
├── .env.example            # Template for environment variables
├── Dockerfile              # Containerization definition
├── pyproject.toml          # Dependency management (Poetry/uv)
└── README.md
```

## 3. Modularity & Dependency Flow

To prevent circular dependencies and ensure testability, the project follows a **Unidirectional Dependency Rule**:

`Infrastructure -> Domain <- API`  
`Infrastructure -> Core`  

**Key Design Decisions:**
1. **Domain Isolation:** The `domain/` package must have **zero** imports from `infrastructure/` or `api/`. It defines interfaces (ABCs) that infrastructure must implement.
2. **Dependency Injection:** High-level modules (`domain/services`) do not instantiate low-level modules (`infrastructure/database`). Instead, implementations are injected at the entry point.
3. **Entry Points:** 
    - `cmd/server/main.py`: Responsible for bootstrapping the app, loading config, and wiring dependencies.
    - `src/main.py`: The application factory (e.g., creating the FastAPI instance).

## 4. Data Shapes

### Domain Model (The Source of Truth)
```python
@dataclass(frozen=True)
class User:
    id: UUID
    email: str
    is_active: bool
```

### API Schema (DTO - Data Transfer Object)
```python
class UserResponse(BaseModel):
    user_id: str
    email: str
```

*Note: We separate Domain Models from API Schemas (DTOs) to allow internal logic changes without breaking public contracts.*

## 5. API Contract (Example)

| Method | Endpoint | Description | Auth | 
|--------|----------|-------------|------| 
| GET    | /v1/users | List users  | JWT  | 
| POST   | /v1/users | Create user | None |