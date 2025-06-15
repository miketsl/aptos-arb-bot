# gh CLI Command Reference for Orchestrator (PM)

This document details the exact `gh` CLI command line formats and structures used by the Orchestrator (PM) mode for managing the `miketsl/aptos-arb-bot` repository and its associated GitHub Project.

**Repository & Project Information (Hardcoded Values):**
*   Owner: `miketsl`
*   Repository: `aptos-arb-bot`
*   Project Number: `2` (Used when a project number is accepted, otherwise use Project ID)
*   Primary Development Branch: `dev`

---

## Issues

### Create an Issue
```bash
gh issue create --repo <owner>/<repo> -t "<title>" -b "<body>"
```
*   Example: `gh issue create --repo miketsl/aptos-arb-bot -t "New Feature" -b "Details about the new feature."`

### View an Issue
```bash
gh issue view <issue_number_or_url> --repo <owner>/<repo> --json state,url,title,body,number
```
*   Example: `gh issue view 21 --repo miketsl/aptos-arb-bot --json state,url,title`

### Close an Issue
```bash
gh issue close <issue_number_or_url> --repo <owner>/<repo> --comment "<closing_comment>"
```
*   Example: `gh issue close 21 --repo miketsl/aptos-arb-bot --comment "Resolved by PR #XX."`

### Create a Development Branch from an Issue
Links the branch to the issue and checks it out locally.
```bash
gh issue develop <issue_number_or_url> --repo <owner>/<repo> --name <branch_name> --base <base_branch>
```
*   `<base_branch>` is typically `dev`.
*   Example: `gh issue develop 21 --repo miketsl/aptos-arb-bot --name feature/task-21-new-docs --base dev`

---

## Branches (Git & GitHub CLI)

### Push New Local Branch to Remote and Set Upstream
(Often handled by `gh issue develop`, but good to know for manual pushes)
```bash
git push -u origin <branch_name>
```

### Delete Local Branch (after merge/closure)
```bash
git branch -d <branch_name>
```

### Delete Remote Branch (after merge/closure)
```bash
git push origin --delete <branch_name>
# OR using gh cli
gh repo sync --branch <branch_name> # This might not be the direct delete command, check gh branch delete if available
# Corrected:
gh branch delete <branch_name> -r <owner>/<repo>
# Note: `gh branch delete` deletes remote and can optionally delete local.
# For just remote: `git push origin --delete <branch_name>` is standard.
# Let's stick to `git` for remote branch deletion for clarity unless `gh` offers a clear advantage for this specific PM workflow.
# For this document, let's use:
# git push origin --delete <branch_name>
```
*   Example: `git push origin --delete feature/task-21-new-docs`

---

## Pull Requests

### Create a Pull Request
```bash
gh pr create --repo <owner>/<repo> --base <base_branch> --head <feature_branch> --title "<title_format>" --body "<body_content>"
```
*   `<base_branch>` is typically `dev`.
*   `<title_format>` example: `feat: Resolve #<issue_number> - <Task Title>`
*   `<body_content>` example: `Closes #<issue_number>.\n\nSummary of changes.`
*   Example: `printf "%s\n\n%s" "Closes #21" "This PR introduces the gh.md file, which documents the gh CLI commands used by the Orchestrator PM mode. This is intended to improve the reliability of CLI usage, as requested in the feedback for the previous task." | gh pr create --repo miketsl/aptos-arb-bot --base dev --head feature/task-21-new-docs --title "docs: Resolve #21 - Add gh.md CLI reference" --body-file -`

### View a Pull Request
```bash
gh pr view <pr_number_or_url> --repo <owner>/<repo> --json state,mergedAt,url,title,body,headRefName
```
*   Example: `gh pr view 25 --repo miketsl/aptos-arb-bot --json state,mergedAt`

### List Pull Requests (e.g., to check if one exists for a branch)
```bash
gh pr list --repo <owner>/<repo> --head <branch_name> --json url,number,title
```
*   Example: `gh pr list --repo miketsl/aptos-arb-bot --head feature/task-21-new-docs --json url`

---

## GitHub Projects (Project V2)

**Important Note on IDs:**
*   **Project Number vs. Project ID:** Some commands accept the project *number* (e.g., `2`), while others require the global *Project ID* (e.g., `PVT_kwHOAP836s4A7hle`).
*   **Item ID:** This is the global ID of the card on the project board (e.g., `PVTI_lAHOAP836s4A7hlezgbf20Q`).
*   **Field ID:** The ID of a specific project field (e.g., "Status").
*   **Option ID:** The ID of a specific option within a single-select field (e.g., the "Done" option within "Status").
*   **Spinner Characters:** When using `jq` to extract IDs, be mindful of spinner characters (like `⣯`) that might be appended to the output if not handled carefully. It's best to grab the full JSON object and then extract the ID if unsure, or pipe output to a command that strips such characters if extracting directly.

### Get Project ID from Project Number
```bash
gh project view <project_number> --owner <owner> --format json --jq .id
```
*   Example: `gh project view 2 --owner miketsl --format json --jq .id` (This will output the Project ID, e.g., `PVT_kwHOAP836s4A7hle`)

### Add Issue/PR to Project
```bash
gh project item-add <project_number_or_id> --owner <owner> --url <issue_or_pr_url>
```
*   Example using project number: `gh project item-add 2 --owner miketsl --url https://github.com/miketsl/aptos-arb-bot/issues/21`

### List Project Items (to find an item's details, including its Item ID)
To get the full JSON object for a specific item based on its content URL (e.g., an issue URL):
```bash
gh project item-list <project_number_or_id> --owner <owner> --format json --jq '.items[] | select(.content.url=="<issue_or_pr_url>")'
```
*   Example: `gh project item-list 2 --owner miketsl --format json --jq '.items[] | select(.content.url=="https://github.com/miketsl/aptos-arb-bot/issues/21")'`
*   From the output of this command, you can find the `"id"` field, which is the **Item ID**.

## Find only items that are not "Done" 
The key fields in the output are:
item.title: The title of the item on the project board.
item.content.number: The issue number.
item.content.url: The URL to the issue.
item.status: The status of the item (e.g., "Todo", "In Progress", "Done").
* Example: `gh project item-list 2 --owner miketsl --format json --jq '.items[] | select(.status != "Done") | {title: .title, number: .content.number, status: .status, url: .content.url, item_id: .id}'`

### Extract only the Item ID
```bash
gh project item-list <project_number_or_id> --owner <owner> --format json --jq '.items[] | select(.content.url=="<issue_or_pr_url>") | .id'
```
*   **Caution:** Ensure spinner characters are not included if copying this output directly.

### List Project Fields (to find Field ID for "Status" and Option IDs for its values)
```bash
gh project field-list <project_number> --owner <owner> --format json
```
*   Example: `gh project field-list 2 --owner miketsl --format json`
*   Parse the JSON output:
    *   Find the object where `name` is "Status". Its `id` is the **Field ID for Status**.
    *   Within that "Status" object, look at the `options` array. Each option (e.g., "Todo", "In Progress", "Done") will have its own `name` and `id` (this is the **Option ID**).

### Edit Project Item Field (e.g., Update Status)
```bash
gh project item-edit --project-id <project_id> --id <item_id> --field-id <field_id_for_status> --single-select-option-id <option_id_for_status_value>
```
*   Example: `gh project item-edit --project-id PVT_kwHOAP836s4A7hle --id PVTI_lAHOAP836s4A7hlezgbf20Q --field-id PVTSSF_lAHOAP836s4A7hlezgvzAkk --single-select-option-id 47fc9ee4` (Sets status to "In Progress" if `47fc9ee4` is the ID for "In Progress")