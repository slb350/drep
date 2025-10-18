#!/usr/bin/env python3
"""
Bulk close all issues in the drep repository on Gitea.
"""

import requests
import time

GITEA_URL = "http://192.168.1.14:3000"
TOKEN = "28a7ee8d662524860667224ec6d35ed50edcc4c0"
REPO_OWNER = "steve"
REPO_NAME = "drep"

headers = {
    "Authorization": f"token {TOKEN}",
    "Content-Type": "application/json"
}

def get_all_open_issues():
    """Fetch all open issues with pagination."""
    all_issues = []
    page = 1
    per_page = 50

    while True:
        url = f"{GITEA_URL}/api/v1/repos/{REPO_OWNER}/{REPO_NAME}/issues"
        params = {
            "state": "open",
            "limit": per_page,
            "page": page
        }

        response = requests.get(url, headers=headers, params=params)
        response.raise_for_status()
        issues = response.json()

        if not issues:
            break

        all_issues.extend(issues)
        print(f"Fetched page {page}: {len(issues)} issues (total so far: {len(all_issues)})")
        page += 1

        # Add small delay to avoid rate limiting
        time.sleep(0.1)

    return all_issues

def close_issue(issue_number):
    """Close a single issue."""
    url = f"{GITEA_URL}/api/v1/repos/{REPO_OWNER}/{REPO_NAME}/issues/{issue_number}"
    data = {"state": "closed"}

    response = requests.patch(url, headers=headers, json=data)
    response.raise_for_status()
    return response.json()

def main():
    print("Fetching all open issues...")
    issues = get_all_open_issues()

    print(f"\nFound {len(issues)} open issues")

    if not issues:
        print("No issues to close!")
        return

    print(f"\nClosing {len(issues)} issues...")
    closed_count = 0

    for issue in issues:
        try:
            close_issue(issue['number'])
            closed_count += 1
            if closed_count % 10 == 0:
                print(f"Closed {closed_count}/{len(issues)} issues...")
        except Exception as e:
            print(f"Error closing issue #{issue['number']}: {e}")

    print(f"\n✓ Successfully closed {closed_count} issues!")

if __name__ == "__main__":
    main()
