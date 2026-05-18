use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const STRIPE_API: &str = "https://api.stripe.com/v1";

fn token() -> anyhow::Result<String> {
    std::env::var("STRIPE_SECRET_KEY")
        .map_err(|_| anyhow::anyhow!("STRIPE_SECRET_KEY not set"))
}

fn client(token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))
        .map(|c| {
            // Store client with basic auth (token as username, empty password)
            let _ = token; // used below
            c
        })?;
    // Rebuild with proper basic auth header
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
    let creds = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        format!("{}:", token),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", creds))
            .map_err(|e| anyhow::anyhow!("invalid token: {}", e))?,
    );
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))
}

fn get(c: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Value> {
    let resp = c.get(url).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Stripe API error {}: {}", status, body["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(body)
}

fn post_form(c: &reqwest::blocking::Client, url: &str, params: &[(&str, &str)]) -> anyhow::Result<Value> {
    let resp = c.post(url).form(params).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Stripe API error {}: {}", status, body["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(body)
}

// ─── stripe_list_customers ───────────────────────────────────────────────────

pub struct StripeListCustomersTool;
impl Tool for StripeListCustomersTool {
    fn name(&self) -> &'static str { "stripe_list_customers" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        let mut url = format!("{}/customers?limit={}", STRIPE_API, limit);
        if let Some(email) = input["email"].as_str() { url.push_str(&format!("&email={}", urlencoding::encode(email))); }
        if let Some(after) = input["starting_after"].as_str() { url.push_str(&format!("&starting_after={}", after)); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeListCustomersTool) }
}

// ─── stripe_get_customer ─────────────────────────────────────────────────────

pub struct StripeGetCustomerTool;
impl Tool for StripeGetCustomerTool {
    fn name(&self) -> &'static str { "stripe_get_customer" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id (Stripe customer ID cus_...)"))?;
        get(&c, &format!("{}/customers/{}", STRIPE_API, id))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeGetCustomerTool) }
}

// ─── stripe_list_payment_intents ─────────────────────────────────────────────

pub struct StripeListPaymentIntentsTool;
impl Tool for StripeListPaymentIntentsTool {
    fn name(&self) -> &'static str { "stripe_list_payment_intents" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        let mut url = format!("{}/payment_intents?limit={}", STRIPE_API, limit);
        if let Some(customer) = input["customer"].as_str() { url.push_str(&format!("&customer={}", customer)); }
        if let Some(after) = input["starting_after"].as_str() { url.push_str(&format!("&starting_after={}", after)); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeListPaymentIntentsTool) }
}

// ─── stripe_get_payment_intent ───────────────────────────────────────────────

pub struct StripeGetPaymentIntentTool;
impl Tool for StripeGetPaymentIntentTool {
    fn name(&self) -> &'static str { "stripe_get_payment_intent" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id (pi_...)"))?;
        get(&c, &format!("{}/payment_intents/{}", STRIPE_API, id))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeGetPaymentIntentTool) }
}

// ─── stripe_list_subscriptions ───────────────────────────────────────────────

pub struct StripeListSubscriptionsTool;
impl Tool for StripeListSubscriptionsTool {
    fn name(&self) -> &'static str { "stripe_list_subscriptions" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        let status = input["status"].as_str().unwrap_or("all");
        let mut url = format!("{}/subscriptions?limit={}&status={}", STRIPE_API, limit, status);
        if let Some(customer) = input["customer"].as_str() { url.push_str(&format!("&customer={}", customer)); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeListSubscriptionsTool) }
}

// ─── stripe_list_products ────────────────────────────────────────────────────

pub struct StripeListProductsTool;
impl Tool for StripeListProductsTool {
    fn name(&self) -> &'static str { "stripe_list_products" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        let active = input["active"].as_bool().map(|a| if a { "&active=true" } else { "&active=false" }).unwrap_or("");
        get(&c, &format!("{}/products?limit={}{}", STRIPE_API, limit, active))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeListProductsTool) }
}

// ─── stripe_list_invoices ────────────────────────────────────────────────────

pub struct StripeListInvoicesTool;
impl Tool for StripeListInvoicesTool {
    fn name(&self) -> &'static str { "stripe_list_invoices" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        let mut url = format!("{}/invoices?limit={}", STRIPE_API, limit);
        if let Some(customer) = input["customer"].as_str() { url.push_str(&format!("&customer={}", customer)); }
        if let Some(status) = input["status"].as_str() { url.push_str(&format!("&status={}", status)); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeListInvoicesTool) }
}

// ─── stripe_create_customer ──────────────────────────────────────────────────

pub struct StripeCreateCustomerTool;
impl Tool for StripeCreateCustomerTool {
    fn name(&self) -> &'static str { "stripe_create_customer" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(email) = input["email"].as_str() { params.push(("email", email.to_string())); }
        if let Some(name) = input["name"].as_str() { params.push(("name", name.to_string())); }
        if let Some(phone) = input["phone"].as_str() { params.push(("phone", phone.to_string())); }
        let param_refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        post_form(&c, &format!("{}/customers", STRIPE_API), &param_refs)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(StripeCreateCustomerTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_stripe_list_customers_tool() -> Box<dyn Tool> { Box::new(StripeListCustomersTool) }
pub fn create_stripe_get_customer_tool() -> Box<dyn Tool> { Box::new(StripeGetCustomerTool) }
pub fn create_stripe_list_payment_intents_tool() -> Box<dyn Tool> { Box::new(StripeListPaymentIntentsTool) }
pub fn create_stripe_get_payment_intent_tool() -> Box<dyn Tool> { Box::new(StripeGetPaymentIntentTool) }
pub fn create_stripe_list_subscriptions_tool() -> Box<dyn Tool> { Box::new(StripeListSubscriptionsTool) }
pub fn create_stripe_list_products_tool() -> Box<dyn Tool> { Box::new(StripeListProductsTool) }
pub fn create_stripe_list_invoices_tool() -> Box<dyn Tool> { Box::new(StripeListInvoicesTool) }
pub fn create_stripe_create_customer_tool() -> Box<dyn Tool> { Box::new(StripeCreateCustomerTool) }
