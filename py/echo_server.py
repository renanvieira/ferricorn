from fastapi import FastAPI
from datetime import datetime
from pydantic import BaseModel
import asyncio

app = FastAPI()

# Data model for JSON requests
class Message(BaseModel):
    content: str

# Simple text echo endpoint
@app.post("/echo/text")
async def echo_text(message: str):
    return {"message": message}

# JSON echo endpoint
@app.post("/echo/json")
async def echo_json(message: Message):
    return {"message": message.content}

# Echo with additional metadata
@app.post("/echo/enhanced")
async def echo_enhanced(message: Message):
    return {
        "message": message.content,
        "length": len(message.content),
        "timestamp": datetime.now().isoformat()
    }

# To run the server:
# uvicorn main:app --reload
