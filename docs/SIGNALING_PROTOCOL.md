# Signaling Protocol (JSON over WebSocket)

The signaling server relays point-to-point messages between registered peers.

## Register

```json
{ "type": "register", "peer_id": "peer-a" }
```

## Offer

```json
{ "type": "offer", "from": "peer-a", "to": "peer-b", "sdp": "..." }
```

## Answer

```json
{ "type": "answer", "from": "peer-b", "to": "peer-a", "sdp": "..." }
```

## Candidate

```json
{ "type": "candidate", "from": "peer-a", "to": "peer-b", "candidate": "..." }
```

## Punch Result

```json
{
  "type": "punch_result",
  "from": "peer-a",
  "to": "peer-b",
  "success": true,
  "reason": null
}
```

## Disconnect

```json
{
  "type": "disconnect",
  "from": "peer-a",
  "to": "peer-b",
  "reason": "session-end"
}
```

## Error (server -> client)

```json
{ "type": "error", "message": "..." }
```
