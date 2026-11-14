//! The mail a test account is given to check the client against: made-up senders on reserved domains, each mail
//! showing one thing a reader has to get right. Every address is under `example.*` or a domain reserved for
//! documentation, so no reply, and no image, can reach anyone.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use std::io::Cursor;

/// A mail, whole, and whether it arrives unread.
pub struct Mail {
    pub literal: String,
    pub unread: bool,
}

const BOUNDARY: &str = "sample-boundary";

fn date(days_ago: i64, hour: u32) -> String {
    let at = Utc::now() - Duration::days(days_ago);
    let at = at.date_naive().and_hms_opt(hour, 12, 0).map_or(at, |time| time.and_utc());
    at.to_rfc2822()
}

/// A header value as RFC 2047 writes text that is not ASCII: `=?UTF-8?B?...?=`.
fn encoded(text: &str) -> String {
    if text.is_ascii() { text.to_string() } else { format!("=?UTF-8?B?{}?=", STANDARD.encode(text)) }
}

/// A name and its address, the name encoded where it needs to be.
fn person(from: &str) -> String {
    match from.split_once(" <") {
        Some((name, address)) => format!("{} <{address}", encoded(name)),
        None => from.to_string(),
    }
}

fn header(from: &str, subject: &str, days_ago: i64, hour: u32, id: &str, extra: &str) -> String {
    format!(
        "From: {}\r\nTo: Test Reader <reader@example.net>\r\nSubject: {}\r\nDate: {}\r\nMessage-Id: <{id}@sample.example>\r\nMIME-Version: 1.0\r\n{extra}",
        person(from),
        encoded(subject),
        date(days_ago, hour)
    )
}

fn base64_lines(bytes: &[u8]) -> String {
    let coded = STANDARD.encode(bytes);
    coded.as_bytes().chunks(76).map(|line| String::from_utf8_lossy(line).into_owned()).collect::<Vec<_>>().join("\r\n")
}

fn html(from: &str, subject: &str, days_ago: i64, hour: u32, id: &str, body: &str, unread: bool) -> Mail {
    let head = header(
        from,
        subject,
        days_ago,
        hour,
        id,
        "Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n",
    );
    Mail { literal: format!("{head}\r\n{}\r\n", base64_lines(body.as_bytes())), unread }
}

/// Who wrote a mail, about what, when and as which message.
struct About<'a> {
    from: &'a str,
    subject: &'a str,
    days_ago: i64,
    hour: u32,
    id: &'a str,
}

fn plain(about: &About<'_>, extra: &str, body: &str, unread: bool) -> Mail {
    let head = header(
        about.from,
        about.subject,
        about.days_ago,
        about.hour,
        about.id,
        &format!("{extra}Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n"),
    );
    Mail { literal: format!("{head}\r\n{}\r\n", base64_lines(body.as_bytes())), unread }
}

/// A blue square, as a picture a mail carries.
fn square_png() -> Vec<u8> {
    let mut png = Vec::new();
    let _ = image::RgbaImage::from_pixel(96, 96, image::Rgba([47, 98, 220, 255]))
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

/// A one-page PDF that says what it is.
fn invoice_pdf() -> Vec<u8> {
    let content = "BT /F1 18 Tf 72 720 Td (Invoice 2026-11, a sample) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (number, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", number + 1).as_bytes());
    }
    let table = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1).as_bytes(),
    );
    pdf
}

fn newsletter() -> Mail {
    let body = r#"<table width="100%" cellpadding="0" cellspacing="0" style="background:#f4f4f7"><tr><td align="center">
<table width="600" cellpadding="24" style="background:#ffffff;font-family:Arial,sans-serif">
<tr><td><img src="https://images.shop.example/banner.png" width="552" height="160" alt="Autumn sale"></td></tr>
<tr><td><h1 style="color:#1d2b5b">Autumn sale: up to 40% off</h1>
<p>Warm jackets, scarves and boots. The offer ends on Sunday.</p>
<p><a href="https://shop.example/sale?utm_source=newsletter&amp;utm_medium=email&amp;utm_campaign=autumn" style="background:#2f62dc;color:#fff;padding:10px 18px;text-decoration:none;border-radius:4px">Shop the sale</a></p>
<table width="100%" cellpadding="8" style="border-collapse:collapse"><tr>
<td style="border:1px solid #ddd"><b>Jacket</b><br>89.00</td><td style="border:1px solid #ddd"><b>Scarf</b><br>19.00</td><td style="border:1px solid #ddd"><b>Boots</b><br>120.00</td>
</tr></table>
<p style="font-size:11px;color:#888">You get this mail as a customer of the Sample Shop. <a href="https://shop.example/unsubscribe?u=sample">Unsubscribe</a></p>
<img src="https://tracker.shop.example/open.gif?u=sample" width="1" height="1" alt="">
</td></tr></table></td></tr></table>"#;
    let mut mail =
        html("Sample Shop <news@shop.example>", "Autumn sale: up to 40% off", 0, 8, "newsletter", body, true);
    mail.literal = mail.literal.replacen(
        "MIME-Version: 1.0\r\n",
        "MIME-Version: 1.0\r\nList-Unsubscribe: <https://shop.example/unsubscribe?u=sample>\r\n",
        1,
    );
    mail
}

fn invoice() -> Mail {
    let head = header(
        "Office <office@example.com>",
        "Invoice 2026-11 and the photo you asked for",
        1,
        10,
        "invoice",
        &format!("Content-Type: multipart/mixed; boundary=\"{BOUNDARY}\"\r\n"),
    );
    let literal = format!(
        "{head}\r\n--{BOUNDARY}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello,\r\n\r\nthe invoice for November and the photo are attached.\r\n\r\nOffice\r\n\
         --{BOUNDARY}\r\nContent-Type: application/pdf; name=\"invoice-2026-11.pdf\"\r\nContent-Disposition: attachment; filename=\"invoice-2026-11.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n\
         --{BOUNDARY}\r\nContent-Type: image/png; name=\"photo.png\"\r\nContent-Disposition: attachment; filename=\"photo.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--{BOUNDARY}--\r\n",
        base64_lines(&invoice_pdf()),
        base64_lines(&square_png())
    );
    Mail { literal, unread: true }
}

fn inline_picture() -> Mail {
    let head = header(
        "Jonas Weber <jonas@example.de>",
        "Grüße aus München – mit Bild",
        2,
        17,
        "inline",
        &format!("Content-Type: multipart/related; boundary=\"{BOUNDARY}\"\r\n"),
    );
    let body = "<p>Hallo,</p><p>hier ist das Bild vom Wochenende, direkt in der Mail:</p><p><img src=\"cid:square@sample.example\" width=\"96\" height=\"96\" alt=\"Ein blaues Quadrat\"></p><p>Schöne Grüße,<br>Jonas</p>";
    let literal = format!(
        "{head}\r\n--{BOUNDARY}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n\
         --{BOUNDARY}\r\nContent-Type: image/png\r\nContent-ID: <square@sample.example>\r\nContent-Disposition: inline; filename=\"square.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--{BOUNDARY}--\r\n",
        base64_lines(body.as_bytes()),
        base64_lines(&square_png())
    );
    Mail { literal, unread: false }
}

fn phishing() -> Mail {
    let body = r#"<p>Dear customer,</p><p>your mailbox is almost full. Sign in within 24 hours or it will be closed:</p>
<p><a href="http://proton-login.attacker.example/verify">https://account.proton.me/login</a></p><p>Proton Team</p>"#;
    html(
        "Proton Support <support@proton-security.example>",
        "Action required: your mailbox will be closed",
        3,
        6,
        "phishing",
        body,
        true,
    )
}

fn long_letter() -> Mail {
    let paragraphs = (1..=30)
        .map(|n| format!("Paragraph {n}. A long letter is read by scrolling, and the reader keeps its place while new mail arrives."))
        .collect::<Vec<_>>()
        .join("\r\n\r\n");
    let about = About {
        from: "Long Reader <long@example.org>",
        subject: "A long letter, to scroll through",
        days_ago: 4,
        hour: 9,
        id: "long",
    };
    plain(&about, "", &paragraphs, false)
}

fn russian() -> Mail {
    let body =
        "Привет!\r\n\r\nЭто письмо на русском, чтобы проверить кириллицу в теме, в имени и в тексте.\r\n\r\nАнна";
    plain(
        &About {
            from: "Анна Петрова <anna@example.org>",
            subject: "Встреча в четверг",
            days_ago: 1,
            hour: 15,
            id: "russian",
        },
        "",
        body,
        true,
    )
}

fn thread() -> Vec<Mail> {
    let first = plain(
        &About {
            from: "Carol Example <carol@example.com>",
            subject: "The plan for Monday",
            days_ago: 5,
            hour: 9,
            id: "thread-1",
        },
        "",
        "Hi,\r\n\r\nshall we meet on Monday at ten?\r\n\r\nCarol",
        false,
    );
    let second = plain(
        &About {
            from: "Dan Example <dan@example.com>",
            subject: "Re: The plan for Monday",
            days_ago: 5,
            hour: 11,
            id: "thread-2",
        },
        "In-Reply-To: <thread-1@sample.example>\r\nReferences: <thread-1@sample.example>\r\n",
        "Ten works for me.\r\n\r\nDan\r\n\r\n> shall we meet on Monday at ten?",
        true,
    );
    vec![first, second]
}

fn quoted_reply() -> Mail {
    let body = "<p>Thanks, that settles it.</p><blockquote style=\"border-left:3px solid #ccc;margin:0;padding-left:12px;color:#555\">\
<p>On Friday Bob wrote:</p><p>The report is ready, with the corrections you asked for.</p></blockquote><p>Eve</p>";
    html("Eve Example <eve@example.com>", "Re: the report, with the corrections", 6, 14, "quoted", body, false)
}

/// Every mail of the set, oldest first.
pub fn all() -> Vec<Mail> {
    let mut mails = vec![quoted_reply()];
    mails.extend(thread());
    mails.extend([long_letter(), phishing(), inline_picture(), invoice(), russian(), newsletter()]);
    mails
}
