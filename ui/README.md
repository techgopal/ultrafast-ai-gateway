# Ultrafast Gateway console

The web console of the gateway: React, TypeScript, Vite, Tailwind CSS and shadcn/ui.
The gateway embeds the build output (`dist/`) and serves it at `/`.

```
pnpm install
pnpm dev         # dev server; /api, /v1 and /health go to a gateway on 127.0.0.1:3900
pnpm lint && pnpm typecheck && pnpm test && pnpm build
```

## Adding a component

shadcn/ui components are source files under `src/components/ui/`, added by the shadcn CLI.
The CLI is not a dependency of this project. Run it with `pnpm dlx`, naming the version:

```
pnpm dlx shadcn@4.21.0 add -y <component>
```

After the CLI has run:

1. If the CLI added the packages `shadcn` or `cn` to `package.json`, remove them
   (`pnpm remove shadcn cn`). A test fails while either is listed or imported.
2. Change the `cn` import of every added component from `"cn"` to `"@/lib/utils"`
   (`import { cn } from "@/lib/utils"`). `cn` there is `twMerge(clsx(inputs))`, from the
   packages `clsx` and `tailwind-merge`. Do not add the `cn` package.
3. In `src/styles/globals.css`, keep `@import "./shadcn.css";`. If the CLI put
   `@import "shadcn/tailwind.css";` back, remove that line.
4. `src/styles/shadcn.css` is a copy of `dist/tailwind.css` of the `shadcn` package.
   If you ran a newer version of the CLI than the one named in the header of that
   file, replace the copy with the file of the new version and update the header.

## The API client

`src/api/schema.d.ts` is generated from `../openapi/admin.json` and committed.
After the API description changed:

```
pnpm gen:api     # writes src/api/schema.d.ts
pnpm check:api   # fails when the committed file is not what gen:api writes
```

`pnpm typecheck` also compiles the generated file on its own, with library checks on
(`tsconfig.api.json`), so a description that gives broken types fails the build.
Pages use the hooks of `src/api/queries.ts`; tests answer the API with MSW from
`src/test/fixtures.ts` and `src/test/handlers.ts`.
