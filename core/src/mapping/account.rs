//! The account, what pays for it, its sessions and sign-in log.

use super::{
    Account, AccountSession, AuthLogEntry, Billing, BillingPlan, Invoice, InvoiceState, Member, MemberState,
    Organization, PLAN_TYPE_BASE, PaymentMethod, Plan, Referrals, Referred, ReferredState, SmtpToken, Value,
};

const MEMBER_ROLE_ADMIN: u64 = 2;

/// A subscription lists its add-ons too: the name people know is the base plan's.
pub(super) fn base_plan(subscription: Option<&Value>) -> Option<&Value> {
    let plans = subscription?.get("Plans")?.as_array()?;
    plans.iter().find(|p| p.get("Type").and_then(Value::as_i64) == Some(PLAN_TYPE_BASE)).or_else(|| plans.first())
}

/// Told apart by what a method holds: Proton's type names change with its payment providers.
pub(super) fn to_payment_method(row: &Value, is_default: bool) -> Option<PaymentMethod> {
    let details = row.get("Details")?;
    let field = |name: &str| details.get(name).and_then(Value::as_str).filter(|value| !value.is_empty());
    let label = match (field("Payer"), field("Brand"), field("Last4")) {
        (Some(payer), ..) => format!("PayPal · {payer}"),
        (None, Some(brand), Some(last4)) => format!("{brand} ···· {last4}"),
        (None, None, Some(last4)) => format!("SEPA ···· {last4}"),
        _ => return None,
    };
    Some(PaymentMethod { label, is_default })
}

pub(super) fn to_payment_methods(rows: &[Value]) -> Vec<PaymentMethod> {
    let mut rows: Vec<&Value> = rows.iter().collect();
    rows.sort_by_key(|row| row.get("Order").and_then(Value::as_i64).unwrap_or(i64::MAX));
    let marked = rows.iter().any(|row| row.get("IsDefault").and_then(Value::as_bool) == Some(true));
    let is_default = |index: usize, row: &Value| {
        if marked { row.get("IsDefault").and_then(Value::as_bool) == Some(true) } else { index == 0 }
    };
    rows.iter().enumerate().filter_map(|(index, row)| to_payment_method(row, is_default(index, row))).collect()
}

pub(super) fn to_invoice(row: &Value) -> Invoice {
    let text = |name: &str| row.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let number = |name: &str| row.get(name).and_then(Value::as_i64).unwrap_or(0);
    Invoice {
        id: text("ID"),
        state: match number("State") {
            0 => InvoiceState::Unpaid,
            1 => InvoiceState::Paid,
            _ => InvoiceState::Other,
        },
        amount: number("AmountDue"),
        currency: text("Currency"),
        created_at: u64::try_from(number("CreateTime")).unwrap_or(0).saturating_mul(1000),
    }
}

pub fn to_billing(user: &Value, subscription: Option<&Value>, methods: &[Value], invoices: &[Value]) -> Billing {
    let text = |v: &Value, name: &str| v.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let number = |v: &Value, name: &str| v.get(name).and_then(Value::as_i64).unwrap_or(0);
    let plan = subscription.zip(base_plan(subscription)).map(|(s, p)| BillingPlan {
        title: text(p, "Title"),
        months: s.get("Cycle").and_then(Value::as_u64).unwrap_or(0),
        amount: number(s, "Amount"),
        renew_amount: number(s, "RenewAmount"),
        currency: text(s, "Currency"),
        renews: number(s, "Renew") != 0,
        period_end: s.get("PeriodEnd").and_then(Value::as_u64).unwrap_or(0).saturating_mul(1000),
    });
    Billing {
        plan,
        credit: number(user, "Credit"),
        currency: text(user, "Currency"),
        methods: to_payment_methods(methods),
        invoices: invoices.iter().map(to_invoice).collect(),
    }
}

pub fn to_account(user: &Value, settings: &Value, subscription: Option<&Value>) -> Account {
    let text = |v: &Value, name: &str| v.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let display = text(user, "DisplayName");
    let plan = base_plan(subscription);
    let number = |v: Option<&Value>, name: &str| v.and_then(|v| v.get(name)).and_then(Value::as_u64).unwrap_or(0);
    Account {
        name: if display.is_empty() { text(user, "Name") } else { display },
        email: text(user, "Email"),
        created_at: number(Some(user), "CreateTime") * 1000,
        paid: number(Some(user), "Subscribed") != 0,
        plan: plan.map(|p| Plan {
            title: text(p, "Title"),
            renews: number(subscription, "Renew") != 0,
            period_end: number(subscription, "PeriodEnd") * 1000,
        }),
        two_factor: match settings.pointer("/2FA/Enabled").and_then(Value::as_u64).unwrap_or(0) {
            0 => "off",
            1 => "totp",
            2 => "fido2",
            _ => "both",
        },
        recovery_email: settings.pointer("/Email/Value").and_then(Value::as_str).unwrap_or_default().to_string(),
        log_mode: number(Some(settings), "LogAuth"),
    }
}

/// Newest first; the server answers oldest first.
#[must_use]
pub fn to_sessions(rows: &[Value], current: Option<&str>) -> Vec<AccountSession> {
    rows.iter()
        .rev()
        .map(|row| {
            let id = row.get("UID").and_then(Value::as_str).unwrap_or_default().to_string();
            AccountSession {
                current: current == Some(id.as_str()),
                id,
                client: row.get("LocalizedClientName").and_then(Value::as_str).unwrap_or_default().to_string(),
                created_at: row.get("CreateTime").and_then(Value::as_u64).unwrap_or(0) * 1000,
                revocable: row.get("Revocable").and_then(Value::as_u64).unwrap_or(0) != 0,
            }
        })
        .collect()
}

pub fn to_auth_log(row: &Value) -> AuthLogEntry {
    let text = |name: &str| row.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    AuthLogEntry {
        time: row.get("Time").and_then(Value::as_u64).unwrap_or(0) * 1000,
        event: row.get("Event").and_then(Value::as_i64).unwrap_or(-1),
        description: text("Description"),
        ip: text("IP"),
        location: text("Location"),
        device: text("Device"),
        failed: text("Status") == "failure",
    }
}

fn count(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

fn seconds_to_ms(value: &Value, name: &str) -> u64 {
    count(value, name).saturating_mul(1000)
}

fn words(value: &Value, name: &str) -> String {
    value.get(name).and_then(Value::as_str).unwrap_or_default().to_string()
}

pub fn to_referrals(link: &str, status: &Value, info: &Value, list: &Value) -> Referrals {
    let rows = list.get("Referrals").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    Referrals {
        link: link.to_string(),
        reward_months: count(status, "RewardMonths"),
        reward_limit: count(status, "RewardMonthsLimit"),
        emails_left: count(status, "EmailsAvailable"),
        currency: words(info, "Currency"),
        for_you: count(info, "ReferrerRewardAmount"),
        for_friend: count(info, "RefereeRewardAmount"),
        most: count(info, "MaxRewardAmount"),
        people: rows
            .iter()
            .map(|row| Referred {
                email: words(row, "Email"),
                created_at: seconds_to_ms(row, "CreateTime"),
                state: match count(row, "State") {
                    0 => ReferredState::Invited,
                    1 => ReferredState::SignedUp,
                    2 => ReferredState::Trial,
                    3 => ReferredState::Completed,
                    _ => ReferredState::Rewarded,
                },
            })
            .collect(),
    }
}

pub fn to_smtp_tokens(reply: &Value) -> Vec<SmtpToken> {
    let rows = reply.get("SmtpTokens").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    rows.iter()
        .map(|row| SmtpToken {
            id: words(row, "SmtpTokenID"),
            name: words(row, "Name"),
            address_id: words(row, "AddressID"),
            created_at: seconds_to_ms(row, "CreateTime"),
            last_used: seconds_to_ms(row, "LastUsedTime"),
        })
        .collect()
}

pub fn to_organization(organization: &Value, members: &Value) -> Organization {
    let rows = members.get("Members").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    Organization {
        name: words(organization, "Name"),
        members: (count(organization, "UsedMembers"), count(organization, "MaxMembers")),
        addresses: (count(organization, "UsedAddresses"), count(organization, "MaxAddresses")),
        domains: (count(organization, "UsedDomains"), count(organization, "MaxDomains")),
        space: (count(organization, "UsedSpace"), count(organization, "MaxSpace")),
        people: rows
            .iter()
            .map(|row| Member {
                name: words(row, "Name"),
                admin: count(row, "Role") == MEMBER_ROLE_ADMIN,
                own: count(row, "Self") != 0,
                state: match count(row, "State") {
                    0 => MemberState::Disabled,
                    2 => MemberState::Invited,
                    _ => MemberState::Enabled,
                },
                used_space: count(row, "UsedSpace"),
                max_space: count(row, "MaxSpace"),
            })
            .collect(),
    }
}
