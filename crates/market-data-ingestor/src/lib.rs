use futures_util::StreamExt;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

// TODO: Consider moving to common crate
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum MarketDataEvent {
    TradeUpdate {
        symbol: String,
        price: f64,
        qty: f64,
        timestamp: u64,
    },
    OrderBookUpdate {
        symbol: String,
        bids: Vec<(f64, f64)>, // (price, quantity)
        asks: Vec<(f64, f64)>, // (price, quantity)
    },
    Heartbeat,
}

#[derive(thiserror::Error, Debug)]
pub enum IngestorError {
    #[error("WebSocket connection error: {0}")]
    ConnectionError(String),
    #[error("WebSocket URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("JSON deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Handler error: {0}")]
    HandlerError(String), // To wrap errors from the handler callback
    #[error("Stream ended unexpectedly")]
    StreamEnded,
}

pub struct WebSocketIngestor {
    url: Url,
    stream: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

impl WebSocketIngestor {
    pub fn new(url_str: String) -> Result<Self, IngestorError> {
        let url = Url::parse(&url_str)?;
        Ok(Self { url, stream: None })
    }

    pub async fn connect(&mut self) -> Result<(), IngestorError> {
        info!("Connecting to WebSocket URL: {}", self.url);
        let (ws_stream, response) = connect_async(self.url.as_str()).await?;
        info!("WebSocket handshake has been successfully completed!");
        info!("Response HTTP Version: {:?}", response.version());
        info!("Response HTTP Status: {}", response.status());
        info!("Response HTTP Headers: {:?}", response.headers());
        self.stream = Some(ws_stream);
        Ok(())
    }

    pub async fn run<F, E>(&mut self, mut handler: F) -> Result<(), IngestorError>
    where
        F: FnMut(MarketDataEvent) -> Result<(), E>,
        E: Debug, // Ensure the error type from handler is Debug
    {
        if self.stream.is_none() {
            self.connect().await?;
        }

        let stream = self.stream.as_mut().ok_or_else(|| {
            IngestorError::ConnectionError("Stream not available after connect attempt".to_string())
        })?;

        info!("Starting to listen for messages from {}", self.url);
        loop {
            tokio::select! {
                Some(msg_result) = stream.next() => {
                    match msg_result {
                        Ok(msg) => {
                            match msg {
                                Message::Text(text) => {
                                    info!("Received text message: {}", text);
                                    match serde_json::from_str::<MarketDataEvent>(&text) {
                                        Ok(event) => {
                                            if let Err(e) = handler(event) {
                                                error!("Handler failed to process event: {:?}", e);
                                                // Depending on requirements, might return IngestorError::HandlerError
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to deserialize message: {}. Raw: {}", e, text);
                                        }
                                    }
                                }
                                Message::Binary(bin) => {
                                    info!("Received binary message: {:?}", bin);
                                    // Placeholder: Handle binary messages if necessary
                                }
                                Message::Ping(ping_data) => {
                                    info!("Received Ping: {:?}", ping_data);
                                    // tokio-tungstenite handles Pongs automatically by default
                                    // If custom Pong is needed:
                                    // if let Err(e) = stream.send(Message::Pong(ping_data)).await {
                                    //     error!("Failed to send Pong: {}", e);
                                    //     return Err(IngestorError::WebSocketError(e));
                                    // }
                                }
                                Message::Pong(pong_data) => {
                                    info!("Received Pong: {:?}", pong_data);
                                }
                                Message::Close(close_frame) => {
                                    info!("Received Close frame: {:?}", close_frame);
                                    return Ok(()); // Connection closed by server
                                }
                                Message::Frame(_frame) => {
                                    // Raw frame, usually not handled directly
                                    info!("Received a raw frame.");
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error reading message from WebSocket: {}", e);
                            // Basic retry: attempt to reconnect once.
                            // For a more robust solution, implement exponential backoff, max retries etc.
                            warn!("Attempting to reconnect...");
                            // For skeleton, just try to connect once more.
                            // More robust retry logic (e.g. exponential backoff) would be needed for production.
                            if let Err(reconnect_err) = self.connect().await {
                                error!("Failed to reconnect: {}", reconnect_err);
                                return Err(IngestorError::WebSocketError(e)); // Return original error
                            }
                            // If reconnect succeeds, the next iteration of the loop will use the new stream.
                            // However, the current 'stream.next()' was on the old, failed stream.
                            // We should probably break or continue to force re-evaluation of stream.
                            // For simplicity in skeleton, we return the original error,
                            // implying the run loop terminates on such an error.
                            error!("Connection lost and reconnect attempted. Terminating run loop with original error: {}", e);
                            return Err(IngestorError::WebSocketError(e));
                        }
                    }
                }
                else => {
                    warn!("WebSocket stream ended.");
                    return Err(IngestorError::StreamEnded);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_data_event_deserialization_trade() {
        let data = r#"{
            "TradeUpdate": {
                "symbol": "BTC/USD",
                "price": 50000.0,
                "qty": 0.5,
                "timestamp": 1678886400000
            }
        }"#;
        let event: MarketDataEvent = serde_json::from_str(data).unwrap();
        assert_eq!(
            event,
            MarketDataEvent::TradeUpdate {
                symbol: "BTC/USD".to_string(),
                price: 50000.0,
                qty: 0.5,
                timestamp: 1678886400000
            }
        );
    }

    #[test]
    fn test_market_data_event_deserialization_orderbook() {
        let data = r#"{
            "OrderBookUpdate": {
                "symbol": "ETH/USD",
                "bids": [[3000.0, 10.0], [2999.5, 5.0]],
                "asks": [[3001.0, 8.0], [3001.5, 12.0]]
            }
        }"#;
        let event: MarketDataEvent = serde_json::from_str(data).unwrap();
        assert_eq!(
            event,
            MarketDataEvent::OrderBookUpdate {
                symbol: "ETH/USD".to_string(),
                bids: vec![(3000.0, 10.0), (2999.5, 5.0)],
                asks: vec![(3001.0, 8.0), (3001.5, 12.0)],
            }
        );
    }

    #[test]
    fn test_market_data_event_deserialization_heartbeat() {
        let data = r#""Heartbeat""#; // Serde represents unit variants as just their name string
        let event: MarketDataEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event, MarketDataEvent::Heartbeat);
    }

    #[test]
    fn test_market_data_event_serialization_trade() {
        let event = MarketDataEvent::TradeUpdate {
            symbol: "BTC/USD".to_string(),
            price: 50000.0,
            qty: 0.5,
            timestamp: 1678886400000,
        };
        // Serde typically wraps enums with named fields in a map with the variant name as key
        let expected_json = r#"{"TradeUpdate":{"symbol":"BTC/USD","price":50000.0,"qty":0.5,"timestamp":1678886400000}}"#;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, expected_json);
    }

    #[test]
    fn test_market_data_event_serialization_orderbook() {
        let event = MarketDataEvent::OrderBookUpdate {
            symbol: "ETH/USD".to_string(),
            bids: vec![(3000.0, 10.0), (2999.5, 5.0)],
            asks: vec![(3001.0, 8.0), (3001.5, 12.0)],
        };
        let expected_json = r#"{"OrderBookUpdate":{"symbol":"ETH/USD","bids":[[3000.0,10.0],[2999.5,5.0]],"asks":[[3001.0,8.0],[3001.5,12.0]]}}"#;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, expected_json);
    }

    #[test]
    fn test_market_data_event_serialization_heartbeat() {
        let event = MarketDataEvent::Heartbeat;
        let expected_json = r#""Heartbeat""#; // Unit variants serialize to their name string
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, expected_json);
    }

    #[tokio::test]
    async fn test_ingestor_new_valid_url() {
        let ingestor = WebSocketIngestor::new("ws://localhost:1234".to_string());
        assert!(ingestor.is_ok());
    }

    #[tokio::test]
    async fn test_ingestor_new_invalid_url() {
        let ingestor = WebSocketIngestor::new("not_a_valid_url".to_string());
        assert!(ingestor.is_err());
        match ingestor.err().unwrap() {
            IngestorError::UrlParseError(_) => {} // Expected
            _ => panic!("Expected UrlParseError"),
        }
    }

    // Note: `connect` and `run` methods are harder to unit test without a live WebSocket server.
    // These would typically be tested with integration tests or by mocking the WebSocket connection.
    // For this skeleton, we'll rely on compilation and the simpler tests above.
}
