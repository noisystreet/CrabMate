use super::Locale;

// --- API / 存储错误（设置、分支、审批等回显）---

pub fn api_err_no_local_storage(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无 localStorage",
        Locale::En => "localStorage is unavailable",
    }
}

pub fn api_err_write_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法写入 api_base",
        Locale::En => "Could not save api_base",
    }
}

pub fn api_err_write_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法写入 model",
        Locale::En => "Could not save model",
    }
}

pub fn api_err_write_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法写入 api_key",
        Locale::En => "Could not save api_key",
    }
}

pub fn api_err_workspace_set_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置失败",
        Locale::En => "Workspace update failed",
    }
}

pub fn api_err_request_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请求失败",
        Locale::En => "Request failed",
    }
}

pub fn api_err_no_response_body(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无响应体",
        Locale::En => "Empty response body",
    }
}

pub fn api_err_branch_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分支请求未成功",
        Locale::En => "Branch request did not succeed",
    }
}

pub fn api_err_approval_failed(l: Locale, status: u16) -> String {
    match l {
        Locale::ZhHans => format!("审批请求失败 {status}"),
        Locale::En => format!("Approval request failed ({status})"),
    }
}
