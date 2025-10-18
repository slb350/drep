#!/bin/bash
# Bulk close all issues in the drep repository on Gitea

GITEA_URL="http://192.168.1.14:3000"
TOKEN="28a7ee8d662524860667224ec6d35ed50edcc4c0"
REPO_OWNER="steve"
REPO_NAME="drep"

echo "Fetching all open issues..."

# Get all open issues (paginate through all pages)
page=1
all_issues=()

while true; do
    issues=$(curl -s -H "Authorization: token $TOKEN" \
        "$GITEA_URL/api/v1/repos/$REPO_OWNER/$REPO_NAME/issues?state=open&limit=50&page=$page")

    # Check if we got any issues
    count=$(echo "$issues" | jq '. | length')

    if [ "$count" -eq 0 ]; then
        break
    fi

    # Extract issue numbers
    issue_numbers=$(echo "$issues" | jq -r '.[].number')
    all_issues+=($issue_numbers)

    echo "Fetched page $page: $count issues (total: ${#all_issues[@]})"
    ((page++))
done

echo ""
echo "Found ${#all_issues[@]} open issues"

if [ ${#all_issues[@]} -eq 0 ]; then
    echo "No issues to close!"
    exit 0
fi

echo "Closing ${#all_issues[@]} issues..."
closed=0

for issue_num in "${all_issues[@]}"; do
    curl -s -X PATCH \
        -H "Authorization: token $TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"state":"closed"}' \
        "$GITEA_URL/api/v1/repos/$REPO_OWNER/$REPO_NAME/issues/$issue_num" > /dev/null

    ((closed++))
    if [ $((closed % 10)) -eq 0 ]; then
        echo "Closed $closed/${#all_issues[@]} issues..."
    fi
done

echo ""
echo "✓ Successfully closed $closed issues!"
