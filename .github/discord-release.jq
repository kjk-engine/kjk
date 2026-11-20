# The #releases message for a published Release (discord-release.yml),
# from the workflow's event. `$role` is a role's id to ping, or "".
# An embed's text holds 4096 characters.
.release as $r
| ($r.body // "" | if length > 3900 then .[:3900] + "…" else . end) as $notes
| (if $r.prerelease
    then "This pre-release is in your kjk launcher when Show pre-releases is turned on in Settings."
    else "This update is available in your kjk launcher."
  end) as $update
| {
    content: (if $role == "" then "" else "<@&\($role)>" end),
    allowed_mentions: { roles: (if $role == "" then [] else [$role] end) },
    embeds: [{
      title: ($r.name // $r.tag_name),
      url: $r.html_url,
      description: $notes,
      color: 5793266,
      timestamp: $r.published_at,
      fields: [{ name: "How to update", value: $update }]
    }]
  }
