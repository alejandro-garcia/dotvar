//! Original SVGL service symbols bundled for offline environment-variable icons.
//! See assets/services/manifest.json for sources and SHA-256 hashes.

macro_rules! service_logos {
    ($( $variant:ident => ($name:literal, $file:literal) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Service { $( $variant, )+ }

        impl Service {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub const fn path(self) -> &'static str {
                match self { $(Self::$variant => concat!("services/", $file),)+ }
            }

            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name,)+ }
            }

            pub const fn monochrome_path(self) -> &'static str {
                match self { $(Self::$variant => concat!("services/monochrome/", $file),)+ }
            }
        }

        pub fn load(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("services/", $file) => Some(include_bytes!(concat!("../assets/services/", $file))),)+
                $(concat!("services/monochrome/", $file) => Some(include_bytes!(concat!("../assets/services/monochrome/", $file))),)+
                _ => None,
            }
        }
    };
}

service_logos! {
    PlanetScale => ("PlanetScale", "planet_scale.svg"),
    BetterAuth => ("Better Auth", "better_auth.svg"),
    Resend => ("Resend", "resend.svg"),
    Vercel => ("Vercel", "vercel.svg"),
    NextJs => ("Next.js", "next_js.svg"),
    Upstash => ("Upstash", "upstash.svg"),
    Supabase => ("Supabase", "supabase.svg"),
    Neon => ("Neon", "neon.svg"),
    Turso => ("Turso", "turso.svg"),
    MongoDb => ("MongoDB", "mongo_db.svg"),
    PostgreSql => ("PostgreSQL", "postgre_sql.svg"),
    MySql => ("MySQL", "my_sql.svg"),
    Redis => ("Redis", "redis.svg"),
    Convex => ("Convex", "convex.svg"),
    Firebase => ("Firebase", "firebase.svg"),
    Appwrite => ("Appwrite", "appwrite.svg"),
    Clerk => ("Clerk", "clerk.svg"),
    Auth0 => ("Auth0", "auth0.svg"),
    AuthJs => ("Auth.js", "auth_js.svg"),
    WorkOs => ("WorkOS", "work_os.svg"),
    Keycloak => ("Keycloak", "keycloak.svg"),
    Stripe => ("Stripe", "stripe.svg"),
    PayPal => ("PayPal", "pay_pal.svg"),
    MercadoPago => ("Mercado Pago", "mercado_pago.svg"),
    Polar => ("Polar", "polar.svg"),
    LemonSqueezy => ("Lemon Squeezy", "lemon_squeezy.svg"),
    DodoPayments => ("Dodo Payments", "dodo_payments.svg"),
    OpenAi => ("OpenAI", "open_ai.svg"),
    Anthropic => ("Anthropic", "anthropic.svg"),
    Gemini => ("Gemini", "gemini.svg"),
    Groq => ("Groq", "groq.svg"),
    Mistral => ("Mistral AI", "mistral.svg"),
    Cohere => ("Cohere", "cohere.svg"),
    Replicate => ("Replicate", "replicate.svg"),
    HuggingFace => ("Hugging Face", "hugging_face.svg"),
    OpenRouter => ("OpenRouter", "open_router.svg"),
    DeepSeek => ("DeepSeek", "deep_seek.svg"),
    Xai => ("xAI", "xai.svg"),
    Perplexity => ("Perplexity AI", "perplexity.svg"),
    Sentry => ("Sentry", "sentry.svg"),
    PostHog => ("PostHog", "post_hog.svg"),
    Axiom => ("Axiom", "axiom.svg"),
    Datadog => ("Datadog", "datadog.svg"),
    Grafana => ("Grafana", "grafana.svg"),
    Plausible => ("Plausible Analytics", "plausible.svg"),
    GoogleAnalytics => ("Google Analytics", "google_analytics.svg"),
    Cloudinary => ("Cloudinary", "cloudinary.svg"),
    Cloudflare => ("Cloudflare", "cloudflare.svg"),
    Aws => ("Amazon Web Services", "aws.svg"),
    GoogleCloud => ("Google Cloud", "google_cloud.svg"),
    Azure => ("Microsoft Azure", "azure.svg"),
    DigitalOcean => ("Digital Ocean", "digital_ocean.svg"),
    Railway => ("Railway", "railway.svg"),
    Render => ("Render", "render.svg"),
    Fly => ("Fly", "fly.svg"),
    Netlify => ("Netlify", "netlify.svg"),
    GitHub => ("GitHub", "git_hub.svg"),
    GitLab => ("GitLab", "git_lab.svg"),
    Google => ("Google", "google.svg"),
    Discord => ("Discord", "discord.svg"),
    Slack => ("Slack", "slack.svg"),
    Twilio => ("Twilio", "twilio.svg"),
    Telegram => ("Telegram", "telegram.svg"),
    Notion => ("Notion", "notion.svg"),
    Linear => ("Linear", "linear.svg"),
    Figma => ("Figma", "figma.svg"),
    Shopify => ("Shopify", "shopify.svg"),
    Typesense => ("Typesense", "typesense.svg"),
    Sanity => ("Sanity", "sanity.svg"),
    Strapi => ("Strapi", "strapi.svg"),
    Payload => ("Payload CMS", "payload.svg"),
    Qdrant => ("Qdrant", "qdrant.svg"),
    Nhost => ("NHost", "nhost.svg"),
    PocketBase => ("Pocketbase", "pocket_base.svg"),
    CockroachDb => ("Cockroach Labs", "cockroach_db.svg"),
    Xata => ("Xata", "xata.svg"),
    Firecrawl => ("Firecrawl", "firecrawl.svg"),
    Dub => ("Dub", "dub.svg"),
    Expo => ("Expo", "expo.svg"),
    Hcaptcha => ("hCaptcha", "hcaptcha.svg"),
}

pub fn paths() -> impl Iterator<Item = &'static str> {
    Service::ALL
        .iter()
        .flat_map(|service| [service.path(), service.monochrome_path()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_symbol(service: Service, size: u32) -> resvg::tiny_skia::Pixmap {
        let tree = resvg::usvg::Tree::from_data(
            load(service.monochrome_path()).unwrap(),
            &resvg::usvg::Options::default(),
        )
        .unwrap();
        assert_eq!(tree.size().width(), 16., "{} root width", service.name());
        assert_eq!(tree.size().height(), 16., "{} root height", service.name());
        let mut image = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(size as f32 / 16., size as f32 / 16.),
            &mut image.as_mut(),
        );
        image
    }

    #[test]
    fn monochrome_symbols_keep_transparency_centering_and_visible_size() {
        for service in Service::ALL {
            let image = render_symbol(*service, 64);
            let mut bounds = (64usize, 64usize, 0usize, 0usize);
            for (i, pixel) in image.pixels().iter().enumerate() {
                // A white alpha mask is required: black-painted counterforms
                // would otherwise become solid foreground again in GPUI.
                assert_eq!(pixel.red(), pixel.alpha(), "{} red mask", service.name());
                assert_eq!(
                    pixel.green(),
                    pixel.alpha(),
                    "{} green mask",
                    service.name()
                );
                assert_eq!(pixel.blue(), pixel.alpha(), "{} blue mask", service.name());
                if pixel.alpha() > 20 {
                    let (x, y) = (i % 64, i / 64);
                    bounds.0 = bounds.0.min(x);
                    bounds.1 = bounds.1.min(y);
                    bounds.2 = bounds.2.max(x);
                    bounds.3 = bounds.3.max(y);
                }
            }
            let (left, top, right, bottom) = bounds;
            let extent = (right + 1 - left).max(bottom + 1 - top);
            assert!(
                (52..=62).contains(&extent),
                "{} extent: {bounds:?}",
                service.name()
            );
            assert!(
                left > 0 && top > 0 && right < 63 && bottom < 63,
                "{} clipped: {bounds:?}",
                service.name()
            );
            assert!(
                (left + right).abs_diff(63) <= 4 && (top + bottom).abs_diff(63) <= 4,
                "{} not centered: {bounds:?}",
                service.name()
            );
        }
    }

    #[test]
    fn turso_preserves_both_transparent_eye_windows() {
        let image = render_symbol(Service::Turso, 128);
        // The two eye regions join underneath the nose in the source artwork;
        // test their interiors rather than counting closed connected components.
        for (x, y) in [(38, 61), (89, 61)] {
            assert!(
                image.pixels()[y * 128 + x].alpha() < 20,
                "Turso eye became solid"
            );
        }
        assert!(
            image.pixels()[68 * 128 + 64].alpha() > 220,
            "Turso nose disappeared"
        );
    }

    #[test]
    fn every_logo_renders_in_the_native_svg_engine() {
        for path in paths() {
            let tree =
                resvg::usvg::Tree::from_data(load(path).unwrap(), &resvg::usvg::Options::default())
                    .unwrap_or_else(|error| panic!("{path}: {error}"));
            let mut image = resvg::tiny_skia::Pixmap::new(16, 16).unwrap();
            let size = tree.size();
            let scale = 16. / size.width().max(size.height());
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut image.as_mut(),
            );
            assert!(
                image.pixels().iter().any(|p| p.alpha() > 0),
                "empty logo: {}",
                path
            );
        }
    }

    #[test]
    fn every_service_has_a_unique_embedded_svg() {
        let mut paths = std::collections::HashSet::new();
        for service in Service::ALL {
            assert!(paths.insert(service.path()), "duplicate logo path");
            assert!(!service.name().is_empty());
            let bytes = load(service.path()).expect("bundled service logo");
            let source = std::str::from_utf8(bytes).expect("UTF-8 SVG");
            assert!(source.contains("<svg"));
            assert!(source.contains("viewBox="));
            assert!(source.contains("http://www.w3.org/2000/svg"));
        }
        assert_eq!(super::paths().count(), Service::ALL.len() * 2);
        assert!(load("services/unknown.svg").is_none());
    }
}
