kubectl proxy --port=8001 &
curl -X PATCH http://localhost:8001/api/v1/nodes/node02.dev.paas/status -H "Content-Type: application/merge-patch+json" -d '{
    "status": {
        "conditions": [
            {
                "type": "Ready",
                "status": "False",
                    "reason": "MyReason",
                    "message": "my message PLEG"
            }
        ]
    }
}'