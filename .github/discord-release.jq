# The #releases message for a published Release (discord-release.yml),
# from the workflow's event. `$role` is a role's id to ping, or "".
# An embed's text holds 4096 characters and a field 1024.
.release as $r
| ($r.body // "" | if length > 3900 then .[:3900] + "…" else . end) as $notes
| ([$r.assets[] | "[\(.name)](\(.browser_download_url))"] | join("\n")) as $files
| {
    content: (if $role == "" then "" else "<@&\($role)>" end),
    allowed_mentions: { roles: (if $role == "" then [] else [$role] end) },
    embeds: [{
      title: (($r.name // $r.tag_name) + (if $r.prerelease then " (pre-release)" else "" end)),
      url: $r.html_url,
      description: $notes,
      color: 5793266,
      timestamp: $r.published_at,
      fields: (if $files == "" then [] else
        [{ name: "Downloads", value: (if ($files | length) > 1024 then $r.html_url else $files end) }]
      end)
    }]
  }
