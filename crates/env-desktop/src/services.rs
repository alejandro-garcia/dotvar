//! Offline, conservative service identification. Never expands or logs a value.
use crate::service_logos::Service;

struct Rule {
    service: Service,
    names: &'static [&'static str],
    hosts: &'static [&'static str],
}

macro_rules! rules {
    ($($service:ident: [$($name:literal),*] => [$($host:literal),*];)*) => {
        const RULES: &[Rule] = &[$(Rule {
            service: Service::$service,
            names: &[$($name),*],
            hosts: &[$($host),*],
        }),*];
    };
}

// Specific namespaces precede broader ones (Google Cloud before Google, etc.).
// Match complete underscore-delimited namespaces, including public/build wrappers.
rules! {
    PlanetScale: ["PLANETSCALE", "PLANET_SCALE", "PSCALE"] => ["psdb.cloud", "planetscale.com"];
    BetterAuth: ["BETTER_AUTH"] => [];
    Resend: ["RESEND"] => ["resend.com"];
    Upstash: ["UPSTASH", "QSTASH"] => ["upstash.io"];
    Supabase: ["SUPABASE"] => ["supabase.co", "supabase.com", "supabase.net"];
    Neon: ["NEON"] => ["neon.tech"];
    Turso: ["TURSO"] => ["turso.io"];
    MongoDb: ["MONGODB", "MONGO"] => ["mongodb.net"];
    PostgreSql: ["POSTGRES", "POSTGRESQL", "PGHOST", "PGPORT", "PGUSER", "PGPASSWORD", "PGDATABASE"] => [];
    MySql: ["MYSQL"] => [];
    Redis: ["REDIS"] => ["redis.cloud", "redislabs.com"];
    Convex: ["CONVEX"] => ["convex.cloud", "convex.site"];
    Firebase: ["FIREBASE"] => ["firebaseio.com", "firebasedatabase.app", "firebaseapp.com"];
    Appwrite: ["APPWRITE"] => ["appwrite.io"];
    Clerk: ["CLERK"] => ["clerk.accounts.dev", "clerk.com", "clerk.dev"];
    Auth0: ["AUTH0"] => ["auth0.com"];
    AuthJs: ["NEXTAUTH"] => [];
    WorkOs: ["WORKOS", "WORK_OS"] => ["workos.com"];
    Keycloak: ["KEYCLOAK"] => [];
    Stripe: ["STRIPE"] => ["stripe.com"];
    PayPal: ["PAYPAL"] => ["paypal.com"];
    MercadoPago: ["MERCADOPAGO", "MERCADO_PAGO"] => ["mercadopago.com", "mercadopago.com.ar"];
    Polar: ["POLAR"] => ["polar.sh"];
    LemonSqueezy: ["LEMONSQUEEZY", "LEMON_SQUEEZY"] => ["lemonsqueezy.com"];
    DodoPayments: ["DODO_PAYMENTS", "DODO_PAYMENTS_API_KEY", "DODOPAYMENTS"] => ["dodopayments.com"];
    OpenAi: ["OPENAI", "OPEN_AI"] => ["openai.com"];
    Anthropic: ["ANTHROPIC"] => ["anthropic.com"];
    Gemini: ["GEMINI", "GOOGLE_GENERATIVE_AI"] => ["generativelanguage.googleapis.com"];
    Groq: ["GROQ"] => ["groq.com"];
    Mistral: ["MISTRAL"] => ["mistral.ai"];
    Cohere: ["COHERE"] => ["cohere.ai", "cohere.com"];
    Replicate: ["REPLICATE"] => ["replicate.com", "replicate.delivery"];
    HuggingFace: ["HUGGINGFACE", "HUGGING_FACE", "HF_TOKEN", "HF_API_TOKEN"] => ["huggingface.co"];
    OpenRouter: ["OPENROUTER", "OPEN_ROUTER"] => ["openrouter.ai"];
    DeepSeek: ["DEEPSEEK"] => ["deepseek.com"];
    Xai: ["XAI"] => ["x.ai"];
    Perplexity: ["PERPLEXITY", "PPLX"] => ["perplexity.ai"];
    Sentry: ["SENTRY"] => ["sentry.io"];
    PostHog: ["POSTHOG", "POST_HOG"] => ["posthog.com", "posthog.dev"];
    Axiom: ["AXIOM"] => ["axiom.co"];
    Datadog: ["DATADOG", "DD_API_KEY", "DD_APP_KEY", "DD_SITE", "DD_AGENT_HOST"] => ["datadoghq.com", "datadoghq.eu"];
    Grafana: ["GRAFANA"] => ["grafana.net", "grafana.com"];
    Plausible: ["PLAUSIBLE"] => ["plausible.io"];
    GoogleAnalytics: ["GOOGLE_ANALYTICS", "GA_MEASUREMENT_ID", "GA_TRACKING_ID", "GA4_MEASUREMENT_ID"] => ["google-analytics.com"];
    Cloudinary: ["CLOUDINARY"] => ["cloudinary.com"];
    Cloudflare: ["CLOUDFLARE", "CF_API_TOKEN", "CF_ACCOUNT_ID", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "R2_ENDPOINT", "TURNSTILE"] => ["r2.cloudflarestorage.com", "workers.dev", "pages.dev", "cloudflare.com"];
    Aws: ["AWS", "AMAZON_WEB_SERVICES"] => ["amazonaws.com"];
    GoogleCloud: ["GOOGLE_CLOUD", "GCLOUD", "GCP", "GOOGLE_APPLICATION_CREDENTIALS"] => ["run.app", "cloudfunctions.net", "storage.googleapis.com"];
    Azure: ["AZURE"] => ["azure.com", "azurewebsites.net", "windows.net", "azurestaticapps.net"];
    DigitalOcean: ["DIGITALOCEAN", "DIGITAL_OCEAN"] => ["digitaloceanspaces.com", "ondigitalocean.app", "db.ondigitalocean.com"];
    Railway: ["RAILWAY"] => ["railway.app", "railway.internal", "rlwy.net"];
    Render: ["RENDER_API_KEY", "RENDER_SERVICE_ID", "RENDER_EXTERNAL_URL", "RENDER_EXTERNAL_HOSTNAME"] => ["onrender.com"];
    Fly: ["FLY_API_TOKEN", "FLY_APP_NAME", "FLY_REGION", "FLY_ALLOC_ID"] => ["fly.dev"];
    Netlify: ["NETLIFY"] => ["netlify.app", "netlify.com"];
    Vercel: ["VERCEL", "BLOB_READ_WRITE_TOKEN"] => ["vercel.app", "vercel.com", "vercel-storage.com"];
    GitHub: ["GITHUB", "GH_TOKEN"] => ["github.com", "githubusercontent.com"];
    GitLab: ["GITLAB"] => ["gitlab.com"];
    Google: ["GOOGLE"] => ["accounts.google.com", "oauth2.googleapis.com"];
    Discord: ["DISCORD"] => ["discord.com", "discordapp.com"];
    Slack: ["SLACK"] => ["slack.com"];
    Twilio: ["TWILIO"] => ["twilio.com"];
    Telegram: ["TELEGRAM"] => ["api.telegram.org"];
    Notion: ["NOTION"] => ["api.notion.com"];
    Linear: ["LINEAR"] => ["api.linear.app"];
    Figma: ["FIGMA"] => ["api.figma.com"];
    Shopify: ["SHOPIFY"] => ["myshopify.com"];
    Typesense: ["TYPESENSE"] => ["typesense.net"];
    Sanity: ["SANITY"] => ["api.sanity.io", "apicdn.sanity.io"];
    Strapi: ["STRAPI"] => ["strapiapp.com"];
    Payload: ["PAYLOAD_SECRET", "PAYLOAD_PUBLIC_SERVER_URL", "PAYLOAD_CONFIG_PATH"] => [];
    Qdrant: ["QDRANT"] => ["qdrant.io"];
    Nhost: ["NHOST"] => ["nhost.run"];
    PocketBase: ["POCKETBASE", "POCKET_BASE"] => [];
    CockroachDb: ["COCKROACH", "COCKROACHDB"] => ["cockroachlabs.cloud"];
    Xata: ["XATA"] => ["xata.sh", "xata.tech"];
    Firecrawl: ["FIRECRAWL"] => ["firecrawl.dev"];
    Dub: ["DUB_API_KEY", "DUB_WORKSPACE_ID"] => ["api.dub.co"];
    Expo: ["EXPO_TOKEN", "EXPO_ACCESS_TOKEN", "EXPO_PROJECT_ID"] => ["exp.host", "expo.dev"];
    Hcaptcha: ["HCAPTCHA"] => ["hcaptcha.com"];
}

/// Names win over values: BETTER_AUTH_URL remains Better Auth on a Vercel host.
/// Values identify hosted providers before generic database protocols/names.
pub fn detect(name: &str, literal: &str) -> Option<Service> {
    let name = name.to_ascii_uppercase();
    let matches_name = |rule: &&Rule| rule.names.iter().any(|alias| namespace(&name, alias));
    let named = RULES.iter().find(matches_name);
    let specific_name = RULES.iter().filter(matches_name).find(|rule| {
        !matches!(
            rule.service,
            Service::PostgreSql | Service::MySql | Service::Redis | Service::MongoDb
        )
    });
    if let Some(rule) = specific_name {
        return Some(rule.service);
    }
    let value = unquote(literal);
    if let Some(service) = token_service(value) {
        return Some(service);
    }
    // URL parsing prevents a provider name in a password, path or query from matching.
    let url = url::Url::parse(value)
        .ok()
        .filter(|url| url.host_str().is_some())
        .or_else(|| {
            // PGHOST and endpoint fields often contain only a hostname or host:port.
            if !value.is_empty() && !value.contains(['/', '@', '?', '#', '$', ' ', '\n', '\r']) {
                url::Url::parse(&format!("https://{value}")).ok()
            } else {
                None
            }
        });
    if let Some(host) = url.as_ref().and_then(url::Url::host_str) {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if let Some(rule) = RULES.iter().find(|rule| {
            rule.hosts.iter().any(|domain| {
                host.eq_ignore_ascii_case(domain)
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
        }) {
            return Some(rule.service);
        }
    }
    if let Some(rule) = named {
        return Some(rule.service);
    }
    match url.as_ref().map(url::Url::scheme) {
        Some("postgres" | "postgresql") => Some(Service::PostgreSql),
        Some("mysql") => Some(Service::MySql),
        Some("redis" | "rediss") => Some(Service::Redis),
        Some("mongodb" | "mongodb+srv") => Some(Service::MongoDb),
        Some("cloudinary") => Some(Service::Cloudinary),
        _ => None,
    }
}

fn namespace(name: &str, alias: &str) -> bool {
    name.match_indices(alias).any(|(start, _)| {
        (start == 0 || name.as_bytes()[start - 1] == b'_')
            && (start + alias.len() == name.len() || name.as_bytes()[start + alias.len()] == b'_')
    })
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2
        && matches!(value.as_bytes()[0], b'\'' | b'"')
        && value.as_bytes().first() == value.as_bytes().last()
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn token_service(value: &str) -> Option<Service> {
    const TOKENS: &[(&str, Service)] = &[
        ("pscale_api_", Service::PlanetScale),
        ("pscale_pw_", Service::PlanetScale),
        ("sk-ant-", Service::Anthropic),
        ("sk-proj-", Service::OpenAi),
        ("sk-svcacct-", Service::OpenAi),
        ("sk-or-v1-", Service::OpenRouter),
        ("rk_live_", Service::Stripe),
        ("rk_test_", Service::Stripe),
        ("github_pat_", Service::GitHub),
        ("ghp_", Service::GitHub),
        ("gho_", Service::GitHub),
        ("ghs_", Service::GitHub),
        ("glpat-", Service::GitLab),
        ("xoxb-", Service::Slack),
        ("xoxp-", Service::Slack),
        ("phc_", Service::PostHog),
        ("phx_", Service::PostHog),
        ("sntrys_", Service::Sentry),
        ("sb_secret_", Service::Supabase),
        ("sb_publishable_", Service::Supabase),
        ("vercel_blob_rw_", Service::Vercel),
    ];
    TOKENS.iter().find_map(|(prefix, service)| {
        let tail = value.strip_prefix(prefix)?;
        (!tail.is_empty()
            && tail
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')))
        .then_some(*service)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_service_has_a_name_rule_with_public_wrappers() {
        for service in Service::ALL {
            let rule = RULES
                .iter()
                .find(|rule| rule.service == *service)
                .expect("missing rule");
            for alias in rule.names {
                for name in [
                    alias.to_string(),
                    format!("NEXT_PUBLIC_{alias}_KEY"),
                    format!("vite_{alias}_url").to_lowercase(),
                ] {
                    assert_eq!(detect(&name, ""), Some(*service), "{name}");
                }
            }
        }
    }

    #[test]
    fn recognizes_contents_even_when_names_are_generic_and_values_quoted() {
        for (name, value, service) in [
            ("TOKEN", "pscale_api_example", Service::PlanetScale),
            (
                "DATABASE_URL",
                "'postgresql://user:pw@db.project.supabase.co:5432/db'",
                Service::Supabase,
            ),
            (
                "POSTGRES_URL",
                "postgres://user:pw@ep-foo.neon.tech/db",
                Service::Neon,
            ),
            (
                "DATABASE_URL",
                "mysql://user:pw@aws.connect.psdb.cloud/db",
                Service::PlanetScale,
            ),
            (
                "REDIS_URL",
                "rediss://default:pw@red.upstash.io:6379",
                Service::Upstash,
            ),
            ("DATABASE_URL", "libsql://db-org.turso.io", Service::Turso),
            ("URL", "https://EXAMPLE.SUPABASE.CO./", Service::Supabase),
            ("PGHOST", "ep-foo.neon.tech:5432", Service::Neon),
            ("VERCEL_POSTGRES_URL", "", Service::Vercel),
            ("KEY", "\"sk-ant-example\"", Service::Anthropic),
            ("KEY", "sk-or-v1-example", Service::OpenRouter),
            (
                "CONNECTION",
                "postgres://localhost:5432/db",
                Service::PostgreSql,
            ),
            (
                "CONNECTION",
                "mongodb+srv://user:pw@cluster.mongodb.net/db",
                Service::MongoDb,
            ),
        ] {
            assert_eq!(detect(name, value), Some(service), "{name}");
        }
    }

    #[test]
    fn explicit_integration_names_win_and_do_not_leak_into_generic_keys() {
        assert_eq!(
            detect("BETTER_AUTH_URL", "https://app.vercel.app"),
            Some(Service::BetterAuth)
        );
        assert_eq!(
            detect("OPENAI_BASE_URL", "https://openrouter.ai/api/v1"),
            Some(Service::OpenAi)
        );
        for (name, value) in [
            ("DATABASE_URL", ""),
            ("EMAIL_FROM", "person@example.com"),
            ("AUTH_TOKEN", "sk_generic"),
            ("AUTH_SECRET", "abc"),
            ("PAYLOAD_SIZE", "100"),
            ("RENDER_MODE", "server"),
            ("STRIPED_COLOR", "red"),
            ("MYVERCEL_KEY", ""),
            ("SOME_TOKEN", "re_ambiguous"),
            ("API_KEY", "sk_live_ambiguous"),
            ("API_KEY", "sk_test_ambiguous"),
            ("URL", "https://supabase.co.evil.example"),
            ("URL", "https://supabase.co@unknown.example"),
            ("URL", "https://unknown.example/supabase.co?next=neon.tech"),
            ("URL", "https://unknown.example#pscale_api_example"),
            ("TOKEN", "prefix_pscale_api_example"),
            ("TOKEN", "pscale_api_"),
            ("URL", "${SUPABASE_URL}"),
            ("TOKEN", ""),
        ] {
            assert_eq!(detect(name, value), None, "{name}");
        }
    }
}
