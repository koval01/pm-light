use super::*;
use crate::model::{ForwardingState, RecoveryPhrase, SenderList};

fn meta(labels: &[&str]) -> ApiMessageMeta {
    ApiMessageMeta {
        id: "m".into(),
        subject: "S".into(),
        sender: ApiAddr { name: Some(String::new()), address: "a@x.io".into(), is_proton: 0 },
        time: 5,
        unread: 1,
        label_ids: labels.iter().map(std::string::ToString::to_string).collect(),
        ..Default::default()
    }
}

#[test]
fn maps_every_system_folder_both_ways() {
    for (folder, label) in SYSTEM {
        assert_eq!(label_of(folder), *label);
        assert_eq!(folder_of_label(label), Some(*folder));
    }
}

#[test]
fn custom_ids_pass_through() {
    assert_eq!(label_of("xyz=="), "xyz==");
}

#[test]
fn dates_are_milliseconds_and_empty_names_vanish() {
    let m = to_meta(&meta(&["0"]), None);
    assert_eq!(m.date, 5000);
    assert_eq!(m.from.name, None);
    assert!(m.unread);
}

#[test]
fn the_viewed_folder_wins_unless_it_is_a_virtual_one() {
    assert_eq!(to_meta(&meta(&["0", "6"]), Some("archive")).folder, "archive");
    assert_eq!(to_meta(&meta(&["10", "6"]), Some("starred")).folder, "archive");
    assert_eq!(to_meta(&meta(&["5", "4"]), Some("all")).folder, "spam");
}

#[test]
fn stars_are_a_label() {
    assert!(to_meta(&meta(&["10"]), None).starred);
    assert!(!to_meta(&meta(&["0"]), None).starred);
}

#[test]
fn attachments_read_their_content_id_and_disposition() {
    let api = ApiAttachment {
        id: "a".into(),
        headers: HashMap::from([
            ("content-id".to_string(), json!(["<img1>"])),
            ("content-disposition".to_string(), json!("inline; filename=x")),
        ]),
        signature: Some("sig".into()),
        ..Default::default()
    };
    let a = to_attachment(&api);
    assert_eq!(a.content_id.as_deref(), Some("img1"));
    assert!(a.inline);
    assert!(a.signed);
    assert_eq!(a.name, "a");
    assert_eq!(a.mime, "application/octet-stream");
}

#[test]
fn builds_a_list_query() {
    let q = list_query("trash", &ListOptions { offset: Some(100), limit: Some(50), ..Default::default() });
    assert!(q.contains("LabelID=3"));
    assert!(q.contains("Page=2"));
    assert!(q.contains("PageSize=50"));
    assert!(q.contains("Desc=1"));
}

#[test]
fn caps_the_page_size_and_encodes_the_keyword() {
    let q = list_query(
        "inbox",
        &ListOptions {
            limit: Some(500),
            keyword: Some("a b&c".into()),
            unread: Some(true),
            has_attachments: Some(true),
            ..Default::default()
        },
    );
    assert!(q.contains("PageSize=150"));
    assert!(q.contains("Keyword=a+b%26c"));
    assert!(q.contains("Unread=1"));
    assert!(q.contains("Attachments=1"));
}

#[test]
fn counts_use_folder_names() {
    let rows = [
        ApiCount { label_id: "0".into(), total: 9, unread: 2 },
        ApiCount { label_id: "custom".into(), total: 1, unread: 0 },
    ];
    let counts = to_counts(&rows);
    assert_eq!(counts["inbox"].unread, 2);
    assert_eq!(counts["custom"].total, 1);
}

#[test]
fn new_mail_means_growth_only() {
    let inbox = |unread| Counts::from([("inbox".to_string(), Count { total: 9, unread })]);
    assert_eq!(new_mail_count(&inbox(2), &inbox(5)), 3);
    assert_eq!(new_mail_count(&inbox(5), &inbox(2)), 0);
    assert_eq!(new_mail_count(&Counts::new(), &Counts::new()), 0);
}

#[test]
fn reads_toggles_including_the_values_above_one() {
    let s = to_mail_settings(&json!({
        "NextMessageOnMove": 2, "Shortcuts": 1, "AutoSaveContacts": 0,
        "ConfirmLink": 1, "AutoDeleteSpamAndTrashDays": 30, "DelaySendSeconds": 20
    }));
    assert!(s.next_message_on_move && s.shortcuts && s.confirm_links && s.auto_delete_old);
    assert!(!s.auto_save_contacts && !s.hide_remote_images);
    assert_eq!(s.delay_send_seconds, 20);
}

#[test]
fn a_null_setting_is_off() {
    assert!(!to_mail_settings(&json!({"AutoDeleteSpamAndTrashDays": null})).auto_delete_old);
}

#[test]
fn auto_delete_is_sent_in_another_field() {
    let t = toggle("autoDeleteOld").unwrap();
    assert_eq!((t.field, t.on, t.off), ("Days", 30, 0));
}

#[test]
fn billing_reads_what_proton_sends() {
    let user = json!({"Credit": 250, "Currency": "EUR"});
    let subscription = json!({"Cycle": 24, "Amount": 8376, "RenewAmount": 4788, "Currency": "EUR",
        "Renew": 1, "PeriodEnd": 2, "Plans": [
            {"Type": 0, "Title": "Add-on"}, {"Type": 1, "Title": "Mail Plus"}]});
    let invoices = [json!({"ID": "1", "State": 1, "AmountDue": 8376, "Currency": "EUR", "CreateTime": 3})];
    let billing = to_billing(&user, Some(&subscription), &[], &invoices);
    let plan = billing.plan.unwrap();
    assert_eq!((plan.title.as_str(), plan.months, plan.amount), ("Mail Plus", 24, 8376));
    assert_eq!((plan.renew_amount, plan.renews, plan.period_end), (4788, true, 2000));
    assert_eq!((billing.credit, billing.currency.as_str()), (250, "EUR"));
    assert_eq!(billing.invoices[0].state, InvoiceState::Paid);
    assert_eq!(billing.invoices[0].created_at, 3000);
}

#[test]
fn billing_of_a_free_account_has_no_plan() {
    let billing = to_billing(&json!({}), None, &[], &[]);
    assert!(billing.plan.is_none() && billing.methods.is_empty() && billing.invoices.is_empty());
}

#[test]
fn payment_methods_are_told_apart_by_their_details() {
    let label = |details: Value| to_payment_method(&json!({"Details": details}), false).map(|m| m.label);
    assert_eq!(label(json!({"Payer": "a@b.c"})).unwrap(), "PayPal · a@b.c");
    assert_eq!(label(json!({"Brand": "Visa", "Last4": "4242"})).unwrap(), "Visa ···· 4242");
    assert_eq!(label(json!({"AccountName": "A", "Last4": "1234"})).unwrap(), "SEPA ···· 1234");
    assert_eq!(label(json!({"Unknown": true})), None);
}

#[test]
fn the_default_method_is_marked_or_comes_first() {
    let card = |order: i64, default: Option<bool>| {
        let mut row = json!({"Order": order, "Details": {"Brand": "Visa", "Last4": order.to_string()}});
        if let Some(default) = default {
            row["IsDefault"] = json!(default);
        }
        row
    };
    let ordered = to_payment_methods(&[card(2, None), card(1, None)]);
    assert_eq!(ordered[0].label, "Visa ···· 1");
    assert!(ordered[0].is_default && !ordered[1].is_default);
    let marked = to_payment_methods(&[card(1, Some(false)), card(2, Some(true))]);
    assert!(!marked[0].is_default && marked[1].is_default);
}

#[test]
fn invoice_states_follow_proton_s_numbers() {
    let state = |n: i64| to_invoice(&json!({"State": n})).state;
    assert_eq!((state(0), state(1)), (InvoiceState::Unpaid, InvoiceState::Paid));
    assert_eq!((state(2), state(5)), (InvoiceState::Other, InvoiceState::Other));
}

#[test]
fn hostile_billing_numbers_do_not_panic() {
    let row = json!({"State": "x", "AmountDue": null, "CreateTime": i64::MAX});
    assert_eq!(to_invoice(&row).created_at, u64::try_from(i64::MAX).unwrap().saturating_mul(1000));
    let subscription = json!({"PeriodEnd": u64::MAX, "Plans": [{"Title": "x"}]});
    assert!(to_billing(&json!({}), Some(&subscription), &[], &[]).plan.is_some());
}

#[test]
fn the_default_address_goes_first() {
    let ids = ["a".to_string(), "b".to_string(), "c".to_string()];
    assert_eq!(with_default_first(&ids, "b"), ["b", "a", "c"]);
}

#[test]
fn auto_forward_is_bit_35() {
    assert!(is_auto_forwarded(1 << 35));
    assert!(!is_auto_forwarded(1 << 34));
}

#[test]
fn unsubscribe_needs_a_method_and_fills_defaults() {
    assert_eq!(to_unsubscribe(None), None);
    assert_eq!(to_unsubscribe(Some(&ApiUnsubscribe::default())), None);
    let raw = ApiUnsubscribe {
        mailto: Some(ApiMailto { to_list: vec!["u@x.io".into()], ..Default::default() }),
        ..Default::default()
    };
    let methods = to_unsubscribe(Some(&raw)).unwrap();
    assert_eq!(methods.mailto.unwrap().subject, "Unsubscribe");
}

#[test]
fn auto_reply_round_trip() {
    let raw = json!({"IsEnabled": true, "Repeat": 4, "Subject": "", "Message": "<p>x</p>"});
    let reply = to_auto_reply(Some(&raw));
    assert!(reply.enabled && reply.permanent);
    assert_eq!(reply.subject, "Auto");
    assert_eq!(reply.text, "x");
    let sent = from_auto_reply(&reply, "Europe/Kyiv");
    assert_eq!(sent["Repeat"], 4);
    assert_eq!(sent["StartTime"], 0);
    assert_eq!(sent["Zone"], "Europe/Kyiv");
    assert_eq!(sent["Message"], "<p>x</p>");
}

#[test]
fn a_dated_reply_keeps_its_dates() {
    let reply =
        AutoReply { enabled: true, permanent: false, start: 10, end: 20, subject: "S".into(), text: "M".into() };
    let sent = from_auto_reply(&reply, "UTC");
    assert_eq!((sent["StartTime"].as_u64(), sent["EndTime"].as_u64()), (Some(10), Some(20)));
    assert_eq!(sent["Repeat"], 0);
}

#[test]
fn sieve_issues_count_lines_from_one() {
    assert_eq!(issue_text(&json!({"message": "bad", "from": {"line": 2}})), "3: bad");
    assert_eq!(issue_text(&json!({})), "Invalid filter");
}

#[test]
fn read_receipt_rules() {
    let asked = json!({"Disposition-Notification-To": "a"});
    let asked = asked.as_object().unwrap();
    assert!(needs_read_receipt(1, asked));
    assert!(!needs_read_receipt(FLAG_SENT, asked));
    assert!(!needs_read_receipt(FLAG_RECEIPT_SENT, asked));
    assert!(!needs_read_receipt(1, &Map::new()));
}

#[test]
fn account_reads_plan_and_security() {
    let account = to_account(
        &json!({"Name": "ann", "DisplayName": "Ann", "Email": "a@p.me", "CreateTime": 1, "Subscribed": 1}),
        &json!({"2FA": {"Enabled": 3}, "Email": {"Value": "r@x.io"}, "LogAuth": 2}),
        Some(&json!({"PeriodEnd": 2, "Renew": 1, "Plans": [
            {"Title": "Storage", "Type": 0}, {"Title": "Mail Plus", "Type": 1}
        ]})),
    );
    assert_eq!(account.name, "Ann");
    assert_eq!(account.two_factor, "both");
    assert_eq!(account.recovery_email, "r@x.io");
    assert_eq!(account.plan.unwrap().title, "Mail Plus");
}

#[test]
fn a_free_account_has_no_plan() {
    let account = to_account(&json!({"Name": "ann"}), &json!({}), None);
    assert!(!account.paid);
    assert_eq!(account.plan, None);
    assert_eq!(account.two_factor, "off");
}

#[test]
fn sessions_come_newest_first_and_mark_the_current_one() {
    let rows = [
        json!({"UID": "a", "LocalizedClientName": "Web", "CreateTime": 1, "Revocable": 1}),
        json!({"UID": "b", "LocalizedClientName": "Other", "CreateTime": 2, "Revocable": 0}),
    ];
    let sessions = to_sessions(&rows, Some("a"));
    assert_eq!(sessions[0].id, "b");
    assert!(sessions[1].current);
    assert!(!sessions[0].revocable);
}

#[test]
fn a_log_entry_tolerates_missing_fields() {
    let entry = to_auth_log(&json!({"Time": 5, "Event": 0, "Status": "failure", "IP": null}));
    assert_eq!(entry.time, 5000);
    assert_eq!(entry.ip, "");
    assert!(entry.failed);
}

#[test]
fn sender_rules_read_addresses_and_domains() {
    let rule = to_sender_rule(&json!({"ID": "1", "Location": 14, "Email": "a@b.c"})).unwrap();
    assert_eq!((rule.sender.as_str(), rule.list), ("a@b.c", SenderList::Blocked));
    let rule = to_sender_rule(&json!({"ID": "2", "Location": 0, "Domain": "b.c"})).unwrap();
    assert_eq!((rule.sender.as_str(), rule.list), ("b.c", SenderList::Allowed));
    assert_eq!(to_sender_rule(&json!({"ID": "3", "Location": 4, "Email": "s@b.c"})).unwrap().list, SenderList::Spam);
}

#[test]
fn unknown_sender_rules_are_left_out() {
    assert_eq!(to_sender_rule(&json!({"ID": "1", "Location": 99, "Email": "a@b.c"})), None);
    assert_eq!(to_sender_rule(&json!({"ID": "2", "Location": 14})), None);
    assert_eq!(to_sender_rule(&json!({"Location": 70000, "Email": "a@b.c"})), None);
}

#[test]
fn a_typed_sender_is_an_address_or_a_domain() {
    let body = sender_rule_body(" a@b.c ", SenderList::Spam).unwrap();
    assert_eq!(body, json!({"Email": "a@b.c", "Location": 4}));
    let body = sender_rule_body("b.c", SenderList::Blocked).unwrap();
    assert_eq!(body, json!({"Domain": "b.c", "Location": 14}));
    assert_eq!(sender_rule_body("b.c", SenderList::Allowed).unwrap()["Location"], 0);
}

#[test]
fn what_is_neither_is_refused() {
    for sender in ["", "   ", "name", "@b.c", "a@b", "a b@c.d", ".com", "b.", "a@"] {
        assert!(matches!(sender_rule_body(sender, SenderList::Spam), Err(crate::Error::InvalidAddress(_))), "{sender}");
    }
}

#[test]
fn forwarding_reads_each_direction() {
    let out = to_forwarding(&json!({"ID": "1", "ForwardeeEmail": "to@b.c", "Type": 1, "State": 3}), true);
    assert_eq!((out.address.as_str(), out.encrypted, out.state), ("to@b.c", true, ForwardingState::Paused));
    let back = to_forwarding(&json!({"ID": "2", "ForwarderEmail": "from@b.c", "Type": 2, "State": 1}), false);
    assert_eq!((back.address.as_str(), back.encrypted, back.state), ("from@b.c", false, ForwardingState::Active));
    assert_eq!(to_forwarding(&json!({"State": 9}), true).state, ForwardingState::Unknown);
    assert_eq!(to_forwarding(&json!({}), true).state, ForwardingState::Unknown);
}

#[test]
fn a_domain_is_good_only_where_proton_says_so() {
    let domain = to_domain(&json!({"DomainName": "b.c", "VerifyState": 2, "MxState": 3, "SpfState": 3,
        "DKIM": {"State": 4}, "DmarcState": 4}));
    assert!(domain.verified && domain.mx && domain.spf && domain.dkim && domain.dmarc);
    let backup =
        to_domain(&json!({"MxState": 4, "VerifyState": 1, "SpfState": 2, "DKIM": {"State": 3}, "DmarcState": 1}));
    assert!(backup.mx && !backup.verified && !backup.spf && !backup.dkim && !backup.dmarc);
    assert!(!to_domain(&json!({})).mx);
}

#[test]
fn recovery_reads_what_is_set() {
    let settings =
        json!({"Email": {"Value": "a@b.c", "Reset": 1}, "Phone": {"Value": "+1", "Reset": 0}, "DeviceRecovery": 1});
    let recovery = to_recovery(&json!({"MnemonicStatus": 3}), &settings);
    assert_eq!((recovery.email.as_str(), recovery.email_reset), ("a@b.c", true));
    assert_eq!((recovery.phone.as_str(), recovery.phone_reset), ("+1", false));
    assert_eq!(recovery.phrase, RecoveryPhrase::Set);
    assert!(recovery.device_backup);
    let phrase = |n: u64| to_recovery(&json!({"MnemonicStatus": n}), &Value::Null).phrase;
    assert_eq!(
        (phrase(0), phrase(1), phrase(2), phrase(4)),
        (RecoveryPhrase::Off, RecoveryPhrase::Missing, RecoveryPhrase::Outdated, RecoveryPhrase::Missing)
    );
}

#[test]
fn search_terms_reach_the_query_under_protons_names() {
    let options = ListOptions {
        from: Some("ann".into()),
        to: Some("bob".into()),
        subject: Some("big news".into()),
        begin: Some(100),
        end: Some(200),
        starred: Some(true),
        unread: Some(false),
        ..Default::default()
    };
    let query = list_query("inbox", &options);
    for part in ["From=ann", "Recipients=bob", "Subject=big+news", "Begin=100", "End=200", "Starred=1", "Unread=0"] {
        assert!(query.contains(part), "{part} in {query}");
    }
}

#[test]
fn an_invitation_keeps_its_amounts_and_its_state() {
    let status = serde_json::json!({"RewardMonths": 2, "RewardMonthsLimit": 18, "EmailsAvailable": 20});
    let info = serde_json::json!({"Currency": "EUR", "ReferrerRewardAmount": 2000, "RefereeRewardAmount": 1000, "MaxRewardAmount": 10000});
    let list = serde_json::json!({"Referrals": [{"Email": "a@b.c", "CreateTime": 5, "State": 3}]});
    let found = to_referrals("https://pr.tn/x", &status, &info, &list);
    assert_eq!((found.reward_months, found.reward_limit, found.emails_left), (2, 18, 20));
    assert_eq!((found.for_you, found.for_friend, found.most), (2000, 1000, 10000));
    assert_eq!(found.people[0].created_at, 5000);
    assert_eq!(found.people[0].state, ReferredState::Completed);
}

#[test]
fn an_account_without_invitations_gives_empty_answers() {
    let found = to_referrals("", &Value::Null, &Value::Null, &Value::Null);
    assert!(found.link.is_empty() && found.people.is_empty());
}

#[test]
fn a_token_never_used_says_so_with_zero() {
    let reply =
        serde_json::json!({"SmtpTokens": [{"SmtpTokenID": "1", "Name": "Printer", "AddressID": "a", "CreateTime": 7}]});
    let tokens = to_smtp_tokens(&reply);
    assert_eq!((tokens[0].created_at, tokens[0].last_used), (7000, 0));
}

#[test]
fn a_member_is_told_apart_by_role_and_state() {
    let members = serde_json::json!({"Members": [{"Name": "A", "Role": 2, "Self": 1, "State": 1}, {"Name": "B", "Role": 1, "State": 2}]});
    let found = to_organization(&serde_json::json!({"Name": "Org", "UsedMembers": 2, "MaxMembers": 5}), &members);
    assert_eq!(found.members, (2, 5));
    assert!(found.people[0].admin && found.people[0].own);
    assert_eq!(found.people[1].state, MemberState::Invited);
}

#[test]
fn a_setting_goes_out_only_with_a_value_of_its_table() {
    assert_eq!(choice_change("sign", 1), Some(("/mail/v4/settings/sign".to_string(), serde_json::json!({"Sign": 1}))));
    assert_eq!(choice_change("sign", 2), None);
    assert_eq!(choice_change("../../core/v4/users", 0), None);
}

#[test]
fn a_setting_that_is_not_set_goes_out_as_null() {
    assert_eq!(
        choice_change("spamAction", 0),
        Some(("/mail/v4/settings/spam-action".to_string(), serde_json::json!({"SpamAction": null})))
    );
}

#[test]
fn a_draft_type_is_sent_and_read_by_different_names() {
    let found = read_choices(&serde_json::json!({"DraftMIMEType": "text/plain"}));
    assert_eq!(found.get("draftType"), Some(&1));
    assert_eq!(
        choice_change("draftType", 1).map(|(_, body)| body),
        Some(serde_json::json!({"MIMEType": "text/plain"}))
    );
}

#[test]
fn a_value_outside_the_table_stands_nowhere() {
    assert_eq!(read_choices(&serde_json::json!({"PMSignature": 2})).get("protonFooter"), None);
}

#[test]
fn a_missing_answer_means_unset_for_what_may_be_unset() {
    assert_eq!(read_choices(&serde_json::json!({})).get("blockConfirmation"), Some(&0));
}

#[test]
fn subscriptions_are_read_as_bits() {
    let found =
        to_user_choices(&serde_json::json!({"News": 5, "Telemetry": 1, "BreachAlerts": {"Eligible": 1, "Value": 0}}));
    assert_eq!(found.news, 5);
    assert!(found.telemetry && found.breach_alerts_offered && !found.breach_alerts);
}

#[test]
fn habits_come_from_the_account() {
    let found = to_habits(&serde_json::json!({
        "PageSize": 100, "ShowMoved": 3, "AlmostAllMail": 1, "EnableFolderColor": 1, "ComposerMode": 1,
        "RightToLeft": 1, "FontFace": "Georgia", "FontSize": 16, "BlockSenderConfirmation": 1, "SpamAction": 1,
    }));
    assert_eq!(found.page_size, 100);
    assert!(found.moved_in_drafts && found.moved_in_sent && found.almost_all_mail && found.folder_colors);
    assert!(found.wide_composer && found.right_to_left && found.skip_block_question && found.unsubscribe_on_spam);
    assert_eq!((found.font_face.as_deref(), found.font_size), (Some("Georgia"), Some(16)));
}

#[test]
fn embedded_pictures_are_hidden_only_when_asked() {
    assert!(to_habits(&serde_json::json!({"HideEmbeddedImages": 1})).hide_embedded);
    assert!(!to_habits(&serde_json::json!({"HideEmbeddedImages": 0})).hide_embedded);
}

#[test]
fn image_metadata_is_a_flag_of_its_own() {
    assert!(to_habits(&serde_json::json!({"RemoveImageMetadata": true})).remove_metadata);
    assert!(!to_habits(&serde_json::json!({"RemoveImageMetadata": false})).remove_metadata);
}

#[test]
fn proton_s_line_is_the_first_bit_of_its_setting() {
    let footer = |value: u8| to_habits(&serde_json::json!({"PMSignature": value})).proton_footer;
    assert!(!footer(0) && footer(1) && !footer(2) && footer(3));
}

#[test]
fn a_plan_can_lock_proton_s_line() {
    let locked = |value: u8| to_habits(&serde_json::json!({"PMSignature": value})).proton_footer_locked;
    assert!(!locked(0) && !locked(1) && locked(2) && locked(3));
}

#[test]
fn nothing_said_means_the_usual_habits() {
    assert_eq!(to_habits(&serde_json::json!({})), Habits::default());
}

#[test]
fn a_page_stays_within_what_the_list_gives() {
    assert_eq!(to_habits(&serde_json::json!({"PageSize": 200})).page_size, 150);
}

#[test]
fn a_font_outside_the_table_never_reaches_a_letter() {
    let found = to_habits(&serde_json::json!({"FontFace": "x\"><script>", "FontSize": 99999}));
    assert_eq!((found.font_face, found.font_size), (None, None));
}

#[test]
fn folders_are_read_from_the_list_the_account_chose() {
    let wide = Habits { moved_in_drafts: true, moved_in_sent: true, almost_all_mail: true, ..Habits::default() };
    assert_eq!(listed_label("drafts", &wide), "1");
    assert_eq!(listed_label("sent", &wide), "2");
    assert_eq!(listed_label("all", &wide), "15");
    assert_eq!(listed_label("inbox", &wide), "0");
    assert_eq!(listed_label("drafts", &Habits::default()), "8");
    assert_eq!(listed_label("all", &Habits::default()), "5");
}

fn in_a_thread(id: &str, date: u64, unread: bool, labels: &[&str]) -> MessageMeta {
    MessageMeta {
        date,
        unread,
        labels: labels.iter().map(ToString::to_string).collect(),
        ..to_meta(&ApiMessageMeta { id: id.into(), ..meta(&[]) }, None)
    }
}

#[test]
fn a_conversation_goes_by_what_it_holds_in_the_folder() {
    let row = conversation_row(
        &serde_json::from_value(serde_json::json!({
            "ID": "c1", "Subject": "Hi", "Time": 100, "NumMessages": 4, "NumUnread": 2, "NumAttachments": 3,
            "Senders": [{"Name": "Ann", "Address": "ann@x.io"}, {"Name": "Bob", "Address": "bob@x.io"}],
            "Recipients": [{"Name": "", "Address": "me@x.io"}],
            "Labels": [
                {"ID": "0", "ContextTime": 90, "ContextNumUnread": 0, "ContextNumAttachments": 1},
                {"ID": "10", "ContextTime": 50, "ContextNumUnread": 2},
            ],
        }))
        .unwrap(),
        "inbox",
    );
    assert_eq!((row.id.as_str(), row.conversation_id.as_str()), ("c1", "c1"));
    assert_eq!((row.messages, row.date, row.unread), (4, 90_000, false));
    assert_eq!((row.starred, row.attachment_count), (true, 1));
    assert_eq!(row.from.email, "ann@x.io");
    assert_eq!(row.from.name.as_deref(), Some("Ann, Bob"));
}

#[test]
fn one_sender_several_times_is_one_name() {
    let row = conversation_row(
        &serde_json::from_value(serde_json::json!({
            "ID": "c", "Senders": [{"Name": "Ann", "Address": "a@x.io"}, {"Name": "Ann", "Address": "a@x.io"}],
        }))
        .unwrap(),
        "inbox",
    );
    assert_eq!(row.from.name.as_deref(), Some("Ann"));
}

#[test]
fn a_conversation_with_no_word_on_the_folder_goes_by_its_own_figures() {
    let row = conversation_row(
        &serde_json::from_value(
            serde_json::json!({"ID": "c", "Time": 7, "NumMessages": 1, "NumUnread": 1, "NumAttachments": 2}),
        )
        .unwrap(),
        "inbox",
    );
    assert_eq!((row.date, row.unread, row.attachment_count), (7000, true, 2));
    assert_eq!(row.from.email, "");
}

#[test]
fn a_read_conversation_opens_at_its_newest_mail() {
    let thread =
        [in_a_thread("a", 1, false, &["0"]), in_a_thread("b", 2, false, &["0"]), in_a_thread("c", 3, false, &["0"])];
    assert_eq!(message_to_open(&thread, "inbox").as_deref(), Some("c"));
}

#[test]
fn an_unread_conversation_opens_where_reading_stopped() {
    let thread =
        [in_a_thread("a", 1, false, &["0"]), in_a_thread("b", 2, true, &["0"]), in_a_thread("c", 3, true, &["0"])];
    assert_eq!(message_to_open(&thread, "inbox").as_deref(), Some("b"));
    let new = [in_a_thread("a", 1, true, &["0"]), in_a_thread("b", 2, true, &["0"])];
    assert_eq!(message_to_open(&new, "inbox").as_deref(), Some("a"));
}

#[test]
fn a_draft_is_not_what_a_conversation_opens_at() {
    let thread = [in_a_thread("a", 1, false, &["0"]), in_a_thread("draft", 2, false, &["1", "8"])];
    assert_eq!(message_to_open(&thread, "inbox").as_deref(), Some("a"));
}

#[test]
fn a_conversation_of_drafts_alone_still_opens() {
    assert_eq!(message_to_open(&[in_a_thread("draft", 2, false, &["1", "8"])], "inbox").as_deref(), Some("draft"));
    assert_eq!(message_to_open(&[], "inbox"), None);
}

#[test]
fn in_a_label_a_conversation_opens_at_what_wears_it() {
    let thread = [in_a_thread("a", 1, false, &["0", "mine"]), in_a_thread("b", 2, false, &["0"])];
    assert_eq!(message_to_open(&thread, "mine").as_deref(), Some("a"));
}

#[test]
fn the_order_of_the_answer_does_not_matter() {
    let thread = [in_a_thread("new", 9, false, &["0"]), in_a_thread("old", 1, false, &["0"])];
    assert_eq!(message_to_open(&thread, "inbox").as_deref(), Some("new"));
}

#[test]
fn view_mode_is_only_conversations_when_said_so() {
    assert!(to_habits(&serde_json::json!({"ViewMode": 0})).conversations);
    assert!(!to_habits(&serde_json::json!({"ViewMode": 1})).conversations);
    assert!(!to_habits(&serde_json::json!({})).conversations);
}

/// When Proton deletes a mail by itself: a message carries it, a conversation carries it per folder, and only the
/// folder the row stands in counts.
#[test]
fn the_time_proton_deletes_a_mail_is_read() {
    let message: ApiMessageMeta =
        serde_json::from_value(serde_json::json!({"ID": "m", "ExpirationTime": 1_765_000_000})).unwrap();
    assert_eq!(to_meta(&message, Some("trash")).expires, 1_765_000_000);
    let kept: ApiMessageMeta = serde_json::from_value(serde_json::json!({"ID": "m"})).unwrap();
    assert_eq!(to_meta(&kept, None).expires, 0);
    let conversation: ApiConversation = serde_json::from_value(serde_json::json!({
        "ID": "c",
        "Labels": [{"ID": "3", "ContextExpirationTime": 1_765_000_000}, {"ID": "0", "ContextExpirationTime": 0}],
    }))
    .unwrap();
    assert_eq!(conversation_row(&conversation, "trash").expires, 1_765_000_000);
    assert_eq!(conversation_row(&conversation, "inbox").expires, 0);
}
