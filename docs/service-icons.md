# Service icon detection

Dotvar recognizes 79 services from the variable name and literal value in memory. It renders bundled SVGs without making network requests or logging values. Detection also works when values are hidden and while editing a draft.

## Precedence

1. An explicit provider namespace in the key, matched case-insensitively at underscore boundaries. Prefixes such as `NEXT_PUBLIC_`, `VITE_`, and `NUXT_PUBLIC_` are supported.
2. A distinctive credential prefix or a known hostname in the value. Detection strips a matching outer pair of quotes for inspection and leaves the document bytes untouched.
3. A generic database name or URL scheme such as `POSTGRES`, `postgresql://`, `mysql://`, `redis://`, or `mongodb+srv://`.
4. The generic file icon when the available signals are ambiguous.

For example, `BETTER_AUTH_URL=https://app.vercel.app` matches Better Auth by name, whereas a `POSTGRES_URL` whose hostname ends in `.neon.tech` matches Neon. Host matching accepts an exact domain or subdomain. Text in URL credentials, paths, queries, and comments does not count as a hostname.

## Service identifiers

Namespaces can occur within longer underscore-delimited keys. Domains include their subdomains.

| Service | Namespaces or names | Domains |
| --- | --- | --- |
| PlanetScale | `PLANETSCALE`, `PLANET_SCALE`, `PSCALE` | `psdb.cloud`, `planetscale.com` |
| Better Auth | `BETTER_AUTH` | — |
| Resend | `RESEND` | `resend.com` |
| Upstash | `UPSTASH`, `QSTASH` | `upstash.io` |
| Supabase | `SUPABASE` | `supabase.co`, `supabase.com`, `supabase.net` |
| Neon | `NEON` | `neon.tech` |
| Turso | `TURSO` | `turso.io` |
| MongoDB | `MONGODB`, `MONGO` | `mongodb.net` |
| PostgreSQL | `POSTGRES`, `POSTGRESQL`, `PGHOST`, `PGPORT`, `PGUSER`, `PGPASSWORD`, `PGDATABASE` | — |
| MySQL | `MYSQL` | — |
| Redis | `REDIS` | `redis.cloud`, `redislabs.com` |
| Convex | `CONVEX` | `convex.cloud`, `convex.site` |
| Firebase | `FIREBASE` | `firebaseio.com`, `firebasedatabase.app`, `firebaseapp.com` |
| Appwrite | `APPWRITE` | `appwrite.io` |
| Clerk | `CLERK` | `clerk.accounts.dev`, `clerk.com`, `clerk.dev` |
| Auth0 | `AUTH0` | `auth0.com` |
| Auth.js | `NEXTAUTH` | — |
| WorkOS | `WORKOS`, `WORK_OS` | `workos.com` |
| Keycloak | `KEYCLOAK` | — |
| Stripe | `STRIPE` | `stripe.com` |
| PayPal | `PAYPAL` | `paypal.com` |
| Mercado Pago | `MERCADOPAGO`, `MERCADO_PAGO` | `mercadopago.com`, `mercadopago.com.ar` |
| Polar | `POLAR` | `polar.sh` |
| Lemon Squeezy | `LEMONSQUEEZY`, `LEMON_SQUEEZY` | `lemonsqueezy.com` |
| Dodo Payments | `DODO_PAYMENTS`, `DODO_PAYMENTS_API_KEY`, `DODOPAYMENTS` | `dodopayments.com` |
| OpenAI | `OPENAI`, `OPEN_AI` | `openai.com` |
| Anthropic | `ANTHROPIC` | `anthropic.com` |
| Gemini | `GEMINI`, `GOOGLE_GENERATIVE_AI` | `generativelanguage.googleapis.com` |
| Groq | `GROQ` | `groq.com` |
| Mistral AI | `MISTRAL` | `mistral.ai` |
| Cohere | `COHERE` | `cohere.ai`, `cohere.com` |
| Replicate | `REPLICATE` | `replicate.com`, `replicate.delivery` |
| Hugging Face | `HUGGINGFACE`, `HUGGING_FACE`, `HF_TOKEN`, `HF_API_TOKEN` | `huggingface.co` |
| OpenRouter | `OPENROUTER`, `OPEN_ROUTER` | `openrouter.ai` |
| DeepSeek | `DEEPSEEK` | `deepseek.com` |
| xAI | `XAI` | `x.ai` |
| Perplexity AI | `PERPLEXITY`, `PPLX` | `perplexity.ai` |
| Sentry | `SENTRY` | `sentry.io` |
| PostHog | `POSTHOG`, `POST_HOG` | `posthog.com`, `posthog.dev` |
| Axiom | `AXIOM` | `axiom.co` |
| Datadog | `DATADOG`, `DD_API_KEY`, `DD_APP_KEY`, `DD_SITE`, `DD_AGENT_HOST` | `datadoghq.com`, `datadoghq.eu` |
| Grafana | `GRAFANA` | `grafana.net`, `grafana.com` |
| Plausible Analytics | `PLAUSIBLE` | `plausible.io` |
| Google Analytics | `GOOGLE_ANALYTICS`, `GA_MEASUREMENT_ID`, `GA_TRACKING_ID`, `GA4_MEASUREMENT_ID` | `google-analytics.com` |
| Cloudinary | `CLOUDINARY` | `cloudinary.com` |
| Cloudflare | `CLOUDFLARE`, `CF_API_TOKEN`, `CF_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_ENDPOINT`, `TURNSTILE` | `r2.cloudflarestorage.com`, `workers.dev`, `pages.dev`, `cloudflare.com` |
| Amazon Web Services | `AWS`, `AMAZON_WEB_SERVICES` | `amazonaws.com` |
| Google Cloud | `GOOGLE_CLOUD`, `GCLOUD`, `GCP`, `GOOGLE_APPLICATION_CREDENTIALS` | `run.app`, `cloudfunctions.net`, `storage.googleapis.com` |
| Microsoft Azure | `AZURE` | `azure.com`, `azurewebsites.net`, `windows.net`, `azurestaticapps.net` |
| Digital Ocean | `DIGITALOCEAN`, `DIGITAL_OCEAN` | `digitaloceanspaces.com`, `ondigitalocean.app`, `db.ondigitalocean.com` |
| Railway | `RAILWAY` | `railway.app`, `railway.internal`, `rlwy.net` |
| Render | `RENDER_API_KEY`, `RENDER_SERVICE_ID`, `RENDER_EXTERNAL_URL`, `RENDER_EXTERNAL_HOSTNAME` | `onrender.com` |
| Fly | `FLY_API_TOKEN`, `FLY_APP_NAME`, `FLY_REGION`, `FLY_ALLOC_ID` | `fly.dev` |
| Netlify | `NETLIFY` | `netlify.app`, `netlify.com` |
| Vercel | `VERCEL`, `BLOB_READ_WRITE_TOKEN` | `vercel.app`, `vercel.com`, `vercel-storage.com` |
| GitHub | `GITHUB`, `GH_TOKEN` | `github.com`, `githubusercontent.com` |
| GitLab | `GITLAB` | `gitlab.com` |
| Google | `GOOGLE` | `accounts.google.com`, `oauth2.googleapis.com` |
| Discord | `DISCORD` | `discord.com`, `discordapp.com` |
| Slack | `SLACK` | `slack.com` |
| Twilio | `TWILIO` | `twilio.com` |
| Telegram | `TELEGRAM` | `api.telegram.org` |
| Notion | `NOTION` | `api.notion.com` |
| Linear | `LINEAR` | `api.linear.app` |
| Figma | `FIGMA` | `api.figma.com` |
| Shopify | `SHOPIFY` | `myshopify.com` |
| Typesense | `TYPESENSE` | `typesense.net` |
| Sanity | `SANITY` | `api.sanity.io`, `apicdn.sanity.io` |
| Strapi | `STRAPI` | `strapiapp.com` |
| Payload CMS | `PAYLOAD_SECRET`, `PAYLOAD_PUBLIC_SERVER_URL`, `PAYLOAD_CONFIG_PATH` | — |
| Qdrant | `QDRANT` | `qdrant.io` |
| NHost | `NHOST` | `nhost.run` |
| Pocketbase | `POCKETBASE`, `POCKET_BASE` | — |
| Cockroach Labs | `COCKROACH`, `COCKROACHDB` | `cockroachlabs.cloud` |
| Xata | `XATA` | `xata.sh`, `xata.tech` |
| Firecrawl | `FIRECRAWL` | `firecrawl.dev` |
| Dub | `DUB_API_KEY`, `DUB_WORKSPACE_ID` | `api.dub.co` |
| Expo | `EXPO_TOKEN`, `EXPO_ACCESS_TOKEN`, `EXPO_PROJECT_ID` | `exp.host`, `expo.dev` |
| hCaptcha | `HCAPTCHA` | `hcaptcha.com` |

## Value prefixes

The prefix must be at the start of the token and followed by ASCII letters, numbers, underscores, or hyphens.

| Service | Prefixes |
| --- | --- |
| PlanetScale | `pscale_api_`, `pscale_pw_` |
| Anthropic | `sk-ant-` |
| OpenAI | `sk-proj-`, `sk-svcacct-` |
| OpenRouter | `sk-or-v1-` |
| Stripe | `rk_live_`, `rk_test_` |
| GitHub | `github_pat_`, `ghp_`, `gho_`, `ghs_` |
| GitLab | `glpat-` |
| Slack | `xoxb-`, `xoxp-` |
| PostHog | `phc_`, `phx_` |
| Sentry | `sntrys_` |
| Supabase | `sb_secret_`, `sb_publishable_` |
| Vercel | `vercel_blob_rw_` |

Shared prefixes such as `sk_live_`, `sk_test_`, and `re_` are ambiguous on their own, as are generic keys such as `AUTH_SECRET` and `DATABASE_URL`. See [`services.rs`](../crates/env-desktop/src/services.rs) for the matching rules and tests.

## Environment colors

| File | Icon color |
| --- | --- |
| Default | `#79716B` |
| `.env.local` | `#008236` |
| `.env.staging` | `#BB4D00` |
| `.env.production` | `#E7000B` |

Environment names are matched as whole filename segments. Production takes precedence in `.env.production.local`. All service marks and the generic fallback use the same environment color for a given file.

Open a copy of the [synthetic example](../examples/service-icons.env) to see the icons. For provenance and an atlas command, see the [SVG catalog](../crates/env-desktop/assets/services/README.md) and [monochrome icons](../crates/env-desktop/assets/services/monochrome/README.md).
