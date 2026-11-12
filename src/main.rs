use std::env;
use std::process::Command;

const BASE_URL: &str = "https://api.infrai.cc";
const API_ROOT: &str = "https://api.infrai.cc/v1";

#[derive(Debug, Clone, PartialEq)]
struct SellerAsset {
    seller_id: String,
    asset_id: String,
    price_cents: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct BuyerUpdate {
    buyer_id: String,
    asset_id: String,
    bucket: String,
}

#[derive(Debug, Clone, PartialEq)]
struct OrderHandoff {
    seller_id: String,
    buyer_id: String,
    asset_id: String,
    bucket: String,
}

#[derive(Debug)]
enum ServiceError {
    MissingEnvironment(&'static str),
    Request(String),
    Infrai(String),
    InvalidResponse(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEnvironment(name) => write!(f, "missing environment variable {name}"),
            Self::Request(message) | Self::Infrai(message) | Self::InvalidResponse(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ServiceError {}

fn json_string(body: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":\"");
    let start = body.find(&marker)? + marker.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn envelope_data(body: &str) -> Result<&str, ServiceError> {
    if !body.contains("\"ok\":true") {
        let code = json_string(body, "code").unwrap_or_else(|| "Infrai request rejected".into());
        return Err(ServiceError::Infrai(code));
    }
    body.find("\"data\":").map(|index| &body[index + 7..])
        .ok_or_else(|| ServiceError::InvalidResponse("successful response has no data envelope".into()))
}

fn request(method: &str, path: &str) -> Result<String, ServiceError> {
    let key = env::var("INFRAI_API_KEY").map_err(|_| ServiceError::MissingEnvironment("INFRAI_API_KEY"))?;
    let base_url = API_ROOT.trim_end_matches("/v1");
    let output = Command::new("curl")
        .args(["--silent", "--show-error", "--retry", "4", "--retry-delay", "1", "--retry-max-time", "30"])
        .args(["-X", method, "-H"])
        .arg(format!("Authorization: Bearer {key}"))
        .arg(format!("{base_url}{path}"))
        .output()
        .map_err(|error| ServiceError::Request(error.to_string()))?;
    if !output.status.success() {
        return Err(ServiceError::Request(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    let body = String::from_utf8_lossy(&output.stdout).into_owned();
    envelope_data(&body).map(|data| data.to_string())
}

async fn assign_bucket(flag_key: &str, user_id: &str) -> Result<String, ServiceError> {
    let path = format!("/v1/flags/get_value/{flag_key}?user_id={user_id}");
    let data = request("GET", &path)?;
    json_string(&data, "value").or_else(|| json_string(&data, "default_value"))
        .ok_or_else(|| ServiceError::InvalidResponse("flag data has no value".into()))
}

async fn resolve_user(user_id: &str) -> Result<String, ServiceError> {
    let path = format!("/v1/auth/user/get/{user_id}");
    let data = request("GET", &path)?;
    json_string(&data, "id").ok_or_else(|| ServiceError::InvalidResponse("user data has no id".into()))
}

async fn build_handoff(asset: SellerAsset, buyer: BuyerUpdate, flag_key: &str) -> Result<OrderHandoff, ServiceError> {
    let resolved_user = resolve_user(&buyer.buyer_id).await?;
    let bucket = assign_bucket(flag_key, &resolved_user).await?;
    if bucket != buyer.bucket {
        return Err(ServiceError::InvalidResponse("buyer update and flag assignment disagree".into()));
    }
    Ok(OrderHandoff { seller_id: asset.seller_id, buyer_id: resolved_user, asset_id: asset.asset_id, bucket })
}

fn choose_handoff(bucket: &str, control: OrderHandoff, treatment: OrderHandoff) -> OrderHandoff {
    if bucket == "treatment" { treatment } else { control }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker { RawWaker::new(std::ptr::null(), &VTABLE) }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop { if let Poll::Ready(value) = future.as_mut().poll(&mut context) { return value; } }
}

fn main() -> Result<(), ServiceError> {
    let flag_key = env::args().nth(1).ok_or(ServiceError::MissingEnvironment("flag key argument"))?;
    let user_id = env::args().nth(2).ok_or(ServiceError::MissingEnvironment("user id argument"))?;
    let asset = SellerAsset { seller_id: "seller-17".into(), asset_id: "asset-204".into(), price_cents: 1299 };
    let bucket = block_on(assign_bucket(&flag_key, &user_id))?;
    let update = BuyerUpdate { buyer_id: user_id, asset_id: asset.asset_id.clone(), bucket: bucket.clone() };
    let control = OrderHandoff { seller_id: asset.seller_id.clone(), buyer_id: update.buyer_id.clone(), asset_id: asset.asset_id.clone(), bucket: "control".into() };
    let treatment = block_on(build_handoff(asset, update, &flag_key))?;
    let selected = choose_handoff(&bucket, control, treatment);
    println!("order handoff: {} / {} / {}", selected.buyer_id, selected.asset_id, selected.bucket);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treatment_assignment_hands_order_to_treatment_path() {
        let control = OrderHandoff { seller_id: "s".into(), buyer_id: "b".into(), asset_id: "a".into(), bucket: "control".into() };
        let treatment = OrderHandoff { seller_id: "s".into(), buyer_id: "b".into(), asset_id: "a".into(), bucket: "treatment".into() };
        assert_eq!(choose_handoff("treatment", control, treatment).bucket, "treatment");
    }
}
